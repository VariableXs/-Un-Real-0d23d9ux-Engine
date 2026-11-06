//! UNX-F10020 B21 收口小结与预算核销 · 0x91500段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10021-J1 冲刺口径成文落账（盘/分/偿/核四段），口径歧义 10/10 次检出
//!
//! 设计要点：
//!   ① 收口口径按四段闭集成文：盘（盘点）、分（分级）、偿（清偿）、
//!      核（核销）。任何一段缺席都不得出小结；歧义口径（未知段名、
//!      空段名）10/10 次显性拒绝，不许静默归并到相邻段。
//!   ② 预算账本按段分册：每段 allocated/spent/written/remaining 四账，
//!      守恒式 allocated = spent + written + remaining 恒成立；核销
//!      只认票号，票号必须落在该段自己的 0x10 窗口内，跨段票一律拒。
//!   ③ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；账只增不减，
//!      任何被拒核销都不得改动分册余额（拒绝不留痕）。
//!   ④ 小结成文是纯函数：同账本两次成文逐字节一致，用 FNV-1a 双跑
//!      指纹复核；成文必须携带四段标记，缺段即 MissingSeg。
//!   ⑤ 错误族与 Linux errno 逐码对照：本模块只允许表内码，表外码
//!      显性拒绝；Windows 近似码仅作说明不参与判定。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10020 独占 0x40 窗口；B21 收口批次起点）。
pub const SEG_BASE: u32 = 0x91500;

/// 四段口径数量（盘/分/偿/核）。
pub const SEG_COUNT: usize = 4;

/// 单段票号窗口宽度（段内票号偏移 0x01..=0x0F 合法）。
pub const TICKET_WINDOW: u32 = 0x10;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 单段预算上限（防止 u32 溢出的工程约束）。
pub const SEG_CAP: u32 = 1_000_000;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseCode {
    /// 口径段名无法解析（不在四段闭集内）
    UnknownSeg,
    /// 空段名
    EmptyTag,
    /// 核销额为零或越过段上限
    BadAmount,
    /// 核销超额：written 越过 allocated - spent
    OverWriteOff,
    /// 重复票号
    DupTicket,
    /// 票号不属于目标段窗口
    ForeignTicket,
    /// 小结缺段：四段必须全部有分配额
    MissingSeg,
    /// 成文为空
    EmptySummary,
    /// 账目倒退（只增不减被破坏）
    LedgerRegress,
    /// 未知票号查账失败
    UnknownTicket,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl CloseCode {
    /// 码值（段基址 + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CloseCode::UnknownSeg => "口径段名无法解析被拒：段名必须落在 盘/分/偿/核 \
                四段闭集内；请改用 pan/fen/chang/he 或单字母 p/f/c/h",
            CloseCode::EmptyTag => "空段名被拒：口径不能为空串；请先确认四段口径再落账",
            CloseCode::BadAmount => "核销额非法被拒：金额必须大于 0 且不超过段预算上限；\
                请复核核销单金额",
            CloseCode::OverWriteOff => "核销超额被拒：written 不得越过 allocated - spent；\
                请先扩预算或削减核销单",
            CloseCode::DupTicket => "重复票号被拒：同一票号只能核销一次；请换新票号重开核销单",
            CloseCode::ForeignTicket => "跨段票被拒：票号必须落在目标段自己的 0x10 窗口内；\
                请按段号重开票据",
            CloseCode::MissingSeg => "小结缺段被拒：盘/分/偿/核四段必须全部有分配额才能成文；\
                请先补齐缺席段的预算分配",
            CloseCode::EmptySummary => "空小结被拒：成文结果不得为空串；请检查账本是否为空册",
            CloseCode::LedgerRegress => "账目倒退被拒：四账只增不减；请排查回滚路径是否误改累计值",
            CloseCode::UnknownTicket => "未知票号被拒：查账时票号不在已核销清单内；\
                请先用核销返回值留存票号",
            CloseCode::NotReproducible => "双跑不一致：同账本两次成文不同；请排查隐藏状态或非确定排序",
            CloseCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [CloseCode; 12] = [
        CloseCode::UnknownSeg,
        CloseCode::EmptyTag,
        CloseCode::BadAmount,
        CloseCode::OverWriteOff,
        CloseCode::DupTicket,
        CloseCode::ForeignTicket,
        CloseCode::MissingSeg,
        CloseCode::EmptySummary,
        CloseCode::LedgerRegress,
        CloseCode::UnknownTicket,
        CloseCode::NotReproducible,
        CloseCode::SegmentViolation,
    ];
}

// ── 口径段解析器 ──────────────────────────────────────────────────────
/// 四段口径闭集（盘/分/偿/核）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegTag {
    /// 盘：存量盘点
    Pan,
    /// 分：存量分级
    Fen,
    /// 偿：清偿执行
    Chang,
    /// 核：预算核销
    He,
}

impl SegTag {
    /// 段索引（账本下标）。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 长名（成文用）。
    pub fn long(self) -> &'static str {
        match self {
            SegTag::Pan => "盘",
            SegTag::Fen => "分",
            SegTag::Chang => "偿",
            SegTag::He => "核",
        }
    }

    /// 闭集全量。
    pub const ALL: [SegTag; 4] = [SegTag::Pan, SegTag::Fen, SegTag::Chang, SegTag::He];
}

/// 口径解析：长名（pan/fen/chang/he）、单字母（p/f/c/h）双形制，
/// 其余一律 UnknownSeg，空串 EmptyTag。解析器是小结成文的第一道闸。
pub fn parse_segment(tag: &str) -> Result<SegTag, CloseCode> {
    if tag.is_empty() {
        return Err(CloseCode::EmptyTag);
    }
    match tag {
        "pan" | "p" => Ok(SegTag::Pan),
        "fen" | "f" => Ok(SegTag::Fen),
        "chang" | "c" => Ok(SegTag::Chang),
        "he" | "h" => Ok(SegTag::He),
        _ => Err(CloseCode::UnknownSeg),
    }
}

// ── 预算账本 ──────────────────────────────────────────────────────────
/// 单段分册：四账 + 已核销票号清单。
#[derive(Clone, Debug)]
pub struct SegBook {
    /// 分配额
    pub allocated: u32,
    /// 已支出额
    pub spent: u32,
    /// 已核销额
    pub written: u32,
    /// 本段已核销票号（核销偏移，0 表示未用）
    pub tickets: Vec<u32>,
}

impl SegBook {
    const fn empty() -> SegBook {
        SegBook { allocated: 0, spent: 0, written: 0, tickets: Vec::new() }
    }

    /// 余额 = allocated - spent - written（饱和下探为 0）。
    pub fn remaining(&self) -> u32 {
        self.allocated.saturating_sub(self.spent).saturating_sub(self.written)
    }

    /// 分册守恒：allocated = spent + written + remaining。
    pub fn conserved(&self) -> bool {
        self.spent.saturating_add(self.written) <= self.allocated
            && self.remaining() == self.allocated - self.spent - self.written
    }
}

/// B21 收口预算账本：四段分册 + 核销入口。
#[derive(Clone, Debug)]
pub struct BudgetLedger {
    segs: [SegBook; SEG_COUNT],
    /// 全账累计核销笔数（只增）
    write_offs: u32,
}

impl BudgetLedger {
    /// 空册构造。
    pub fn new() -> BudgetLedger {
        BudgetLedger { segs: [SegBook::empty(), SegBook::empty(), SegBook::empty(), SegBook::empty()], write_offs: 0 }
    }

    /// 预算分配：amount > 0 且各段不越过 SEG_CAP。
    pub fn allocate(&mut self, seg: SegTag, amount: u32) -> Result<(), CloseCode> {
        if amount == 0 || amount > SEG_CAP {
            return Err(CloseCode::BadAmount);
        }
        let book = match self.segs.get_mut(seg.index()) {
            Some(b) => b,
            None => return Err(CloseCode::SegmentViolation),
        };
        let next = book.allocated.saturating_add(amount);
        if next > SEG_CAP {
            return Err(CloseCode::BadAmount);
        }
        book.allocated = next;
        Ok(())
    }

    /// 支出：从余额里扣，超额拒绝（拒绝不留痕）。
    pub fn spend(&mut self, seg: SegTag, amount: u32) -> Result<(), CloseCode> {
        if amount == 0 {
            return Err(CloseCode::BadAmount);
        }
        let book = match self.segs.get_mut(seg.index()) {
            Some(b) => b,
            None => return Err(CloseCode::SegmentViolation),
        };
        if amount > book.remaining() {
            return Err(CloseCode::OverWriteOff);
        }
        book.spent = book.spent.saturating_add(amount);
        Ok(())
    }

    /// 预算核销：票号必须落在该段 0x10 窗口内且未用过；成功后
    /// written 增加 amount、票号入册、write_offs 自增。
    pub fn write_off(&mut self, seg: SegTag, ticket: u32, amount: u32) -> Result<u32, CloseCode> {
        if amount == 0 || amount > SEG_CAP {
            return Err(CloseCode::BadAmount);
        }
        let base = (seg.index() as u32 + 1).wrapping_mul(TICKET_WINDOW);
        let off = ticket.wrapping_sub(base);
        if off == 0 || off >= TICKET_WINDOW {
            return Err(CloseCode::ForeignTicket);
        }
        let book = match self.segs.get_mut(seg.index()) {
            Some(b) => b,
            None => return Err(CloseCode::SegmentViolation),
        };
        let mut i = 0usize;
        while i < book.tickets.len() {
            if let Some(t) = book.tickets.get(i) {
                if *t == off {
                    return Err(CloseCode::DupTicket);
                }
            }
            i += 1;
        }
        if amount > book.remaining() {
            return Err(CloseCode::OverWriteOff);
        }
        book.written = book.written.saturating_add(amount);
        book.tickets.push(off);
        self.write_offs = self.write_offs.saturating_add(1);
        Ok(off)
    }

    /// 查票：票号已核销返回其核销偏移。
    pub fn lookup_ticket(&self, seg: SegTag, ticket: u32) -> Result<u32, CloseCode> {
        let base = (seg.index() as u32 + 1).wrapping_mul(TICKET_WINDOW);
        let off = ticket.wrapping_sub(base);
        if off == 0 || off >= TICKET_WINDOW {
            return Err(CloseCode::ForeignTicket);
        }
        let book = match self.segs.get(seg.index()) {
            Some(b) => b,
            None => return Err(CloseCode::SegmentViolation),
        };
        let mut i = 0usize;
        while i < book.tickets.len() {
            if let Some(t) = book.tickets.get(i) {
                if *t == off {
                    return Ok(off);
                }
            }
            i += 1;
        }
        Err(CloseCode::UnknownTicket)
    }

    /// 全账守恒：四段分册全部守恒。
    pub fn conserved(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < SEG_COUNT {
            if let Some(b) = self.segs.get(i) {
                if !b.conserved() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    }

    /// 分册只读视图。
    pub fn book(&self, seg: SegTag) -> Option<&SegBook> {
        self.segs.get(seg.index())
    }

    /// 累计核销笔数。
    pub fn write_offs(&self) -> u32 {
        self.write_offs
    }
}

// ── 收口小结成文（纯函数） ────────────────────────────────────────────
/// 四段成文：任一段 allocated==0 即缺段拒绝；输出形如
/// 「盘:alloc/spent/written;分:...;偿:...;核:...」的定长口径行。
pub fn compose_summary(l: &BudgetLedger) -> Result<String, CloseCode> {
    let mut i = 0usize;
    while i < SEG_COUNT {
        match l.segs.get(i) {
            Some(b) => {
                if b.allocated == 0 {
                    return Err(CloseCode::MissingSeg);
                }
            }
            None => return Err(CloseCode::SegmentViolation),
        }
        i += 1;
    }
    let mut out = String::new();
    let mut k = 0usize;
    while k < SEG_COUNT {
        let tag = match SegTag::ALL.get(k) {
            Some(t) => *t,
            None => return Err(CloseCode::SegmentViolation),
        };
        let b = match l.book(tag) {
            Some(b) => b,
            None => return Err(CloseCode::SegmentViolation),
        };
        if k > 0 {
            out.push(';');
        }
        out.push_str(tag.long());
        out.push(':');
        let piece = format!("{}/{}/{}", b.allocated, b.spent, b.written);
        out.push_str(&piece);
        k += 1;
    }
    if out.is_empty() {
        return Err(CloseCode::EmptySummary);
    }
    Ok(out)
}

// ── 注入核：坏核销样本矩阵 ────────────────────────────────────────────
/// 四类坏核销样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadWriteOff {
    /// 零额核销
    ZeroAmount,
    /// 超额核销（越过余额）
    OverBudget,
    /// 重复票
    DupTicket,
    /// 跨段票
    ForeignTicket,
}

impl BadWriteOff {
    /// 闭集全量。
    pub const ALL: [BadWriteOff; 4] = [
        BadWriteOff::ZeroAmount,
        BadWriteOff::OverBudget,
        BadWriteOff::DupTicket,
        BadWriteOff::ForeignTicket,
    ];

    /// 期望诊断码。
    pub fn code(self) -> CloseCode {
        match self {
            BadWriteOff::ZeroAmount => CloseCode::BadAmount,
            BadWriteOff::OverBudget => CloseCode::OverWriteOff,
            BadWriteOff::DupTicket => CloseCode::DupTicket,
            BadWriteOff::ForeignTicket => CloseCode::ForeignTicket,
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
    pub fn record(&mut self, caught: bool) -> Result<(), CloseCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(CloseCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏核销注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
/// 每轮用全新账本，确保被拒样本不改分册余额（拒绝不留痕）。
pub fn run_bad_writeoff_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadWriteOff::ALL.len() {
        let case = match BadWriteOff::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = BudgetLedger::new();
            let _ = l.allocate(SegTag::He, 100);
            let _ = l.spend(SegTag::He, 10);
            let caught = match case {
                BadWriteOff::ZeroAmount => l.write_off(SegTag::He, base_ticket(SegTag::He, 1), 0)
                    .err() == Some(CloseCode::BadAmount),
                BadWriteOff::OverBudget => {
                    l.write_off(SegTag::He, base_ticket(SegTag::He, 1), 91)
                        .err() == Some(CloseCode::OverWriteOff)
                }
                BadWriteOff::DupTicket => {
                    let t = base_ticket(SegTag::He, 2);
                    let _ = l.write_off(SegTag::He, t, 5);
                    l.write_off(SegTag::He, t, 5).err() == Some(CloseCode::DupTicket)
                }
                BadWriteOff::ForeignTicket => {
                    l.write_off(SegTag::He, base_ticket(SegTag::Pan, 1), 5)
                        .err() == Some(CloseCode::ForeignTicket)
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

/// 段内第 n 号票的绝对票号（段基址 = (index+1)*0x10）。
pub fn base_ticket(seg: SegTag, n: u32) -> u32 {
    (seg.index() as u32 + 1).wrapping_mul(TICKET_WINDOW).wrapping_add(n)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数/坏金额族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；越权/缺据族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复票号族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；预算超额族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏票/跨段票族"),
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

/// 规范语料双跑指纹：四段分配 → 支出 → 合法核销 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = BudgetLedger::new();
    let mut k = 0usize;
    while k < SEG_COUNT {
        if let Some(t) = SegTag::ALL.get(k) {
            let _ = l.allocate(*t, 100 + k as u32 * 10);
        }
        k += 1;
    }
    let _ = l.spend(SegTag::Pan, 20);
    let _ = l.spend(SegTag::He, 10);
    let _ = l.write_off(SegTag::Chang, base_ticket(SegTag::Chang, 3), 15);
    let bytes = match compose_summary(&l) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10020 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10020_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10020");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91500..=0x91FFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91500..=0x91FFF", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 口径解析器：四段双形制全通过
    let parse_ok = parse_segment("pan") == Ok(SegTag::Pan)
        && parse_segment("fen") == Ok(SegTag::Fen)
        && parse_segment("chang") == Ok(SegTag::Chang)
        && parse_segment("he") == Ok(SegTag::He)
        && parse_segment("p") == Ok(SegTag::Pan)
        && parse_segment("h") == Ok(SegTag::He);
    s.add("口径解析器：四段长短名双形制全通过", parse_ok,
          "盘/分/偿/核的 pan|p、fen|f、chang|c、he|h 形制必须全可解析");

    // 3. 歧义口径注入：未知段与空段 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_segment("dip").is_ok() || parse_segment("").is_ok()
            || parse_segment("PAN") .is_ok() || parse_segment("panx").is_ok() {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("歧义口径注入：未知段/空段/大小写漂移 10/10 拒绝", amb_ok,
          "口径歧义必须逐次显性拒绝，不许静默归并");

    // 4. 预算分配：四段各分 100，合计守恒
    let mut l = BudgetLedger::new();
    let mut alloc_ok = true;
    let mut k = 0usize;
    while k < SEG_COUNT {
        if let Some(t) = SegTag::ALL.get(k) {
            if l.allocate(*t, 100).is_err() {
                alloc_ok = false;
            }
        }
        k += 1;
    }
    s.add("预算分配：四段各 100 且全账守恒", alloc_ok && l.conserved(),
          "分配后每段 allocated=100，守恒式恒成立");

    // 5. 正常流：合法票核销成功且查票可回
    let t3 = base_ticket(SegTag::He, 3);
    let normal = match l.write_off(SegTag::He, t3, 25) {
        Ok(off) => off == 3 && l.lookup_ticket(SegTag::He, t3) == Ok(3),
        Err(_) => false,
    };
    s.add("正常流：段内合法票核销 25 并可回查", normal,
          "合法核销返回核销偏移，且票号入册可查");

    // 6. 注入核：四类坏核销各 10/10 拒绝
    let (inj, per_class) = run_bad_writeoff_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏核销各 10/10 拒绝", all_caught,
          "零额/超额/重复票/跨段票必须逐类逐次拒绝");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadWriteOff::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 拒绝不留痕：被拒核销后分册余额不变
    let mut l2 = BudgetLedger::new();
    let _ = l2.allocate(SegTag::He, 100);
    let before = l2.book(SegTag::He).map(|b| b.remaining()).unwrap_or(0);
    let _ = l2.write_off(SegTag::He, 1, 999);
    let _ = l2.write_off(SegTag::He, base_ticket(SegTag::Pan, 1), 5);
    let after = l2.book(SegTag::He).map(|b| b.remaining()).unwrap_or(0);
    s.add("拒绝不留痕：坏核销不改分册余额", before == after && after == 100,
          "被拒核销不得改动 written/tickets 任何账目");

    // 9. 账目单调：连续核销只增不减
    let mut l3 = BudgetLedger::new();
    let _ = l3.allocate(SegTag::Chang, 200);
    let mut mono = true;
    let mut prev = 0u32;
    let mut n = 1u32;
    while n <= 5 {
        if l3.write_off(SegTag::Chang, base_ticket(SegTag::Chang, n), 10).is_err() {
            mono = false;
        }
        let w = l3.book(SegTag::Chang).map(|b| b.written).unwrap_or(0);
        if w <= prev && n > 1 {
            mono = false;
        }
        prev = w;
        n += 1;
    }
    s.add("账目单调：五连核销 written 只增不减", mono && prev == 50,
          "written 50 = 5×10，任何回退都显性失败");

    // 10. 四段小结成文非空且含四段标记
    let sum = compose_summary(&l);
    let text_ok = match &sum {
        Ok(t) => t.contains("盘") && t.contains("分") && t.contains("偿") && t.contains("核"),
        Err(_) => false,
    };
    s.add("小结成文：四段标记齐全且非空", text_ok,
          "成文必须携带 盘:…;分:…;偿:…;核:… 定长口径行");

    // 11. 缺段拒绝：任一段未分配即 MissingSeg
    let mut l4 = BudgetLedger::new();
    let _ = l4.allocate(SegTag::Pan, 10);
    let _ = l4.allocate(SegTag::Fen, 10);
    let _ = l4.allocate(SegTag::Chang, 10);
    s.add("缺段拒绝：核段缺席 → MissingSeg",
          compose_summary(&l4).err() == Some(CloseCode::MissingSeg),
          "四段闭集缺一即拒绝成文，不许出残缺小结");

    // 12. 成文确定性：同账本两次成文逐字节一致
    let a = compose_summary(&l);
    let b = compose_summary(&l);
    s.add("成文确定性：两次成文逐字节一致", a == b,
          "纯函数成文不得依赖隐藏状态或遍历顺序漂移");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(1) && agrees_with_linux(17)
        && agrees_with_linux(28) && agrees_with_linux(9)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 14. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < CloseCode::ALL.len() {
        let mut j = i + 1;
        while j < CloseCode::ALL.len() {
            if let (Some(a), Some(b)) = (CloseCode::ALL.get(i), CloseCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = CloseCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 15. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同输入两次必须逐位一致，隐藏状态即失败");

    // 16. 票号窗口：段基址互异且合法票落窗内
    let win_ok = base_ticket(SegTag::Pan, 1) == 0x11
        && base_ticket(SegTag::He, 1) == 0x41
        && l.lookup_ticket(SegTag::He, base_ticket(SegTag::Pan, 1))
            .err() == Some(CloseCode::ForeignTicket);
    s.add("票号窗口：四段基址互异且跨段票拒", win_ok,
          "段基址 = (index+1)*0x10，跨段票必须被 ForeignTicket 拒绝");

    // 动态明细（≤8）：账目快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum {
        Ok(t) => t.len(),
        Err(_) => 0,
    };
    let sum_text = format!("小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_segment("pan"), Ok(SegTag::Pan));
        assert_eq!(parse_segment("f"), Ok(SegTag::Fen));
        assert_eq!(parse_segment("chang"), Ok(SegTag::Chang));
        assert_eq!(parse_segment("he"), Ok(SegTag::He));
        assert!(parse_segment("xyz").is_err());
        assert_eq!(parse_segment("").err(), Some(CloseCode::EmptyTag));
    }

    #[test]
    fn allocate_and_conservation() {
        let mut l = BudgetLedger::new();
        for t in SegTag::ALL.iter() {
            assert!(l.allocate(*t, 100).is_ok());
        }
        assert!(l.conserved());
        assert!(l.allocate(SegTag::Pan, 0).is_err());
    }

    #[test]
    fn normal_write_off_roundtrip() {
        let mut l = BudgetLedger::new();
        let _ = l.allocate(SegTag::He, 100);
        let t = base_ticket(SegTag::He, 5);
        assert_eq!(l.write_off(SegTag::He, t, 30), Ok(5));
        assert_eq!(l.lookup_ticket(SegTag::He, t), Ok(5));
        assert_eq!(l.book(SegTag::He).unwrap().remaining(), 70);
    }

    #[test]
    fn bad_writeoff_matrix_green() {
        let (inj, per) = run_bad_writeoff_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn over_budget_rejected_without_trace() {
        let mut l = BudgetLedger::new();
        let _ = l.allocate(SegTag::Pan, 10);
        assert_eq!(l.write_off(SegTag::Pan, base_ticket(SegTag::Pan, 1), 11).err(),
                   Some(CloseCode::OverWriteOff));
        assert_eq!(l.book(SegTag::Pan).unwrap().written, 0);
        assert!(l.book(SegTag::Pan).unwrap().tickets.is_empty());
    }

    #[test]
    fn dup_ticket_detected() {
        let mut l = BudgetLedger::new();
        let _ = l.allocate(SegTag::Fen, 100);
        let t = base_ticket(SegTag::Fen, 2);
        assert!(l.write_off(SegTag::Fen, t, 5).is_ok());
        assert_eq!(l.write_off(SegTag::Fen, t, 5).err(), Some(CloseCode::DupTicket));
    }

    #[test]
    fn missing_seg_blocks_summary() {
        let mut l = BudgetLedger::new();
        let _ = l.allocate(SegTag::Pan, 1);
        assert_eq!(compose_summary(&l).err(), Some(CloseCode::MissingSeg));
    }

    #[test]
    fn summary_contains_all_marks() {
        let mut l = BudgetLedger::new();
        for t in SegTag::ALL.iter() {
            let _ = l.allocate(*t, 10);
        }
        let s = compose_summary(&l).unwrap_or_default();
        assert!(s.contains("盘:") && s.contains("核:"));
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
        let s = run_unx10020_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
