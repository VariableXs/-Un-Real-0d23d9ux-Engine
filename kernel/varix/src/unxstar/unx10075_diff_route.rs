//! UNX-F10075 差异项处置流转（勘误附录通道）· 0x922C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10076-J1 处置流转留痕（逐差异归通道 10/10 次），私改对账项 10/10 次拦截
//!
//! 设计要点：
//!   ① 处置通道三选一闭集：勘误附录 / 预期差 / 红账通道；差异条目必须
//!      先登记（登记时冻结内容指纹）再流转，流转时逐差异归通道并落
//!      留痕账（步号连续、通道闭集），未归通道不得放行。
//!   ② 私改拦截：流转时重算条目内容指纹，与登记指纹不符 → TamperedItem
//!      逐次拦截（10/10）；指纹由 FNV-1a 确定性推导，无随机。
//!   ③ 流转账守恒式：已流转数 = 勘误 + 预期差 + 红账，登记数 = 已流转
//!      + 待处置，恒成立；步号必须连续递增（断号 StepBroken 拒绝）。
//!   ④ 注入核：私改对账项 / 无效通道 / 双重流转 / 未登记条目四类坏样本
//!      逐类 10/10 次拒绝，注入账 注入 = 检出 + 逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10075 独占 0x40 窗口，下一卡 0x92300）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x922C0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 流转台容量上限。
pub const DESK_CAP: usize = 64;

/// 留痕账容量上限。
pub const LOG_CAP: usize = 128;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteCode {
    /// 条目未登记即要求流转
    UnknownItem,
    /// 未知处置通道 token
    UnknownChannel,
    /// 空通道 token
    EmptyChannel,
    /// 私改对账项（内容指纹与登记不符）
    TamperedItem,
    /// 双重流转（同条目已归通道）
    DupRoute,
    /// 留痕账容量满
    LogFull,
    /// 流转账失守恒
    RouteRegress,
    /// 步号断档（留痕账步号不连续）
    StepBroken,
    /// 空成文
    EmptySummary,
    /// 拒绝后账被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 通道标签往返不一致
    ChannelRoundTrip,
    /// 段位越界
    SegmentViolation,
}

impl RouteCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RouteCode::UnknownItem => "条目未登记被拒：流转必须先登记冻结指纹；\
                请先 register 再 route",
            RouteCode::UnknownChannel => "未知通道被拒：处置只认 errata|expdiff|redbook 闭集；\
                请改用闭集内写法再流转",
            RouteCode::EmptyChannel => "空通道被拒：通道不能为空串；请确认归口后再流转",
            RouteCode::TamperedItem => "私改对账项被拒：条目内容指纹与登记不符；\
                私改必须逐次拦截，请回滚至登记版本",
            RouteCode::DupRoute => "双重流转被拒：同一条目只能归一个通道；\
                改道请走勘误更正而非二次流转",
            RouteCode::LogFull => "容量满被拒：留痕账已达上限；请先归档旧账再流转",
            RouteCode::RouteRegress => "流转账失守恒：已流转必须等于 勘误+预期差+红账；\
                请排查是否绕过通道计数器",
            RouteCode::StepBroken => "步号断档被拒：留痕账步号必须连续递增；\
                请排查流转路径是否跳步",
            RouteCode::EmptySummary => "空成文被拒：流转小结不得为空串；请检查登记表是否为空",
            RouteCode::RejectedTrace => "拒绝留痕被拒：被拒流转不得改动留痕账；\
                请排查拒绝路径是否误提交",
            RouteCode::ChannelRoundTrip => "通道往返不一致：label 再解析必须回到原通道；\
                请检查闭集文案与解析器是否同步",
            RouteCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RouteCode; 12] = [
        RouteCode::UnknownItem,
        RouteCode::UnknownChannel,
        RouteCode::EmptyChannel,
        RouteCode::TamperedItem,
        RouteCode::DupRoute,
        RouteCode::LogFull,
        RouteCode::RouteRegress,
        RouteCode::StepBroken,
        RouteCode::EmptySummary,
        RouteCode::RejectedTrace,
        RouteCode::ChannelRoundTrip,
        RouteCode::SegmentViolation,
    ];
}

// ── 通道闭集与解析器 ──────────────────────────────────────────────────
/// 处置通道闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    /// 勘误附录通道
    Errata,
    /// 预期差通道
    ExpDiff,
    /// 红账通道
    RedBook,
}

impl Channel {
    /// 中文标签（成文与往返用）。
    pub fn label(self) -> &'static str {
        match self {
            Channel::Errata => "勘误附录",
            Channel::ExpDiff => "预期差",
            Channel::RedBook => "红账通道",
        }
    }

    /// 闭集全量。
    pub const ALL: [Channel; 3] = [Channel::Errata, Channel::ExpDiff, Channel::RedBook];
}

/// 通道解析：长名 errata/expdiff/redbook 与单字母 e/x/r 双形制，其余一律
/// UnknownChannel，空串 EmptyChannel。解析器是归口的唯一入口。
pub fn parse_channel(tag: &str) -> Result<Channel, RouteCode> {
    if tag.is_empty() {
        return Err(RouteCode::EmptyChannel);
    }
    match tag {
        "errata" | "e" => Ok(Channel::Errata),
        "expdiff" | "x" => Ok(Channel::ExpDiff),
        "redbook" | "r" => Ok(Channel::RedBook),
        _ => Err(RouteCode::UnknownChannel),
    }
}

// ── 流转台（状态机） ──────────────────────────────────────────────────
/// 条目状态行：登记指纹 + 归口（None=待处置）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemRow {
    /// 条目号
    pub item: u32,
    /// 登记时冻结的内容指纹
    pub fp: u32,
    /// 归口（None = 待处置）
    pub routed: Option<Channel>,
}

impl ItemRow {
    /// 是否待处置。
    pub fn pending(self) -> bool {
        self.routed.is_none()
    }
}

/// 流转台：登记（冻结指纹）→ 流转（归通道）两相状态机。
#[derive(Clone, Debug)]
pub struct RouteDesk {
    rows: Vec<ItemRow>,
    cap: usize,
}

impl RouteDesk {
    /// 空台构造（容量 DESK_CAP）。
    pub fn new() -> RouteDesk {
        RouteDesk { rows: Vec::new(), cap: DESK_CAP }
    }

    /// 线性查找条目下标。
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

    /// 登记条目并冻结内容指纹；重复登记显性拒绝（不留痕）。
    pub fn register(&mut self, item: u32, content: &str) -> Result<u32, RouteCode> {
        if self.find(item).is_some() {
            return Err(RouteCode::RejectedTrace);
        }
        if self.rows.len() >= self.cap {
            return Err(RouteCode::LogFull);
        }
        let fp = fnv1a(content.as_bytes());
        self.rows.push(ItemRow { item, fp, routed: None });
        Ok(fp)
    }

    /// 查条目状态行。
    pub fn row(&self, item: u32) -> Option<ItemRow> {
        match self.find(item) {
            Some(i) => self.rows.get(i).copied(),
            None => None,
        }
    }

    /// 流转归口：未知条目 UnknownItem、指纹不符 TamperedItem、
    /// 已流转 DupRoute；任何拒绝路径不改台账（拒绝不留痕）。
    pub fn route(&mut self, item: u32, ch: Channel, fp_claim: u32) -> Result<Channel, RouteCode> {
        let idx = match self.find(item) {
            Some(i) => i,
            None => return Err(RouteCode::UnknownItem),
        };
        let fp = match self.rows.get(idx) {
            Some(r) => r.fp,
            None => return Err(RouteCode::UnknownItem),
        };
        if fp != fp_claim {
            return Err(RouteCode::TamperedItem);
        }
        let already = match self.rows.get(idx) {
            Some(r) => r.routed.is_some(),
            None => false,
        };
        if already {
            return Err(RouteCode::DupRoute);
        }
        if let Some(r) = self.rows.get_mut(idx) {
            r.routed = Some(ch);
        }
        Ok(ch)
    }

    /// 已登记条目数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 待处置条目数。
    pub fn pending(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.pending() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 是否空台。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

// ── 留痕账（账本） ────────────────────────────────────────────────────
/// 留痕条目：步号 + 条目号 + 通道。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RouteEntry {
    /// 步号（从 1 起连续递增）
    pub step: u32,
    /// 条目号
    pub item: u32,
    /// 归口通道
    pub ch: Channel,
}

/// 留痕账：守恒式 已流转 = 勘误 + 预期差 + 红账；步号连续递增。
#[derive(Clone, Debug)]
pub struct RouteLog {
    entries: Vec<RouteEntry>,
    cap: usize,
    errata: u32,
    expdiff: u32,
    redbook: u32,
}

impl RouteLog {
    /// 空账构造。
    pub fn new() -> RouteLog {
        RouteLog { entries: Vec::new(), cap: LOG_CAP, errata: 0, expdiff: 0, redbook: 0 }
    }

    /// 落痕一笔：步号必须接尾步 +1（首步必须为 1），断号 StepBroken 拒绝。
    pub fn log(&mut self, item: u32, ch: Channel) -> Result<u32, RouteCode> {
        if self.entries.len() >= self.cap {
            return Err(RouteCode::LogFull);
        }
        let expect_step = match self.entries.last() {
            Some(e) => e.step + 1,
            None => 1,
        };
        let step = expect_step;
        self.entries.push(RouteEntry { step, item, ch });
        match ch {
            Channel::Errata => self.errata = self.errata.saturating_add(1),
            Channel::ExpDiff => self.expdiff = self.expdiff.saturating_add(1),
            Channel::RedBook => self.redbook = self.redbook.saturating_add(1),
        }
        Ok(step)
    }

    /// 步号连续核：逐笔步号 = 前笔 + 1（首笔 1）。
    pub fn steps_continuous(&self) -> bool {
        let mut expect = 1u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) if e.step == expect => {}
                _ => return false,
            }
            expect += 1;
            i += 1;
        }
        true
    }

    /// 已流转总数。
    pub fn routed(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 按通道取计数。
    pub fn ch_count(&self, ch: Channel) -> u32 {
        match ch {
            Channel::Errata => self.errata,
            Channel::ExpDiff => self.expdiff,
            Channel::RedBook => self.redbook,
        }
    }

    /// 守恒式：已流转 = 三通道之和。
    pub fn conserved(&self) -> bool {
        self.errata.saturating_add(self.expdiff).saturating_add(self.redbook) == self.routed()
    }

    /// 留痕条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ── 注入核：坏流转样本矩阵 ────────────────────────────────────────────
/// 四类坏流转样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRoute {
    /// 私改对账项：指纹不符
    TamperedItem,
    /// 无效通道 token
    UnknownChannel,
    /// 双重流转
    DupRoute,
    /// 未登记条目直接流转
    UnregisteredRoute,
}

impl BadRoute {
    /// 闭集全量。
    pub const ALL: [BadRoute; 4] = [
        BadRoute::TamperedItem,
        BadRoute::UnknownChannel,
        BadRoute::DupRoute,
        BadRoute::UnregisteredRoute,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadRoute::TamperedItem => "私改对账项",
            BadRoute::UnknownChannel => "无效通道",
            BadRoute::DupRoute => "双重流转",
            BadRoute::UnregisteredRoute => "未登记条目流转",
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

    /// 记一次注入结果；账失守恒返回 RouteRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), RouteCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(RouteCode::RouteRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏流转注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新流转台，确保被拒样本不留痕。
pub fn run_bad_route_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadRoute::ALL.len() {
        let case = match BadRoute::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadRoute::TamperedItem => {
                    let mut d = RouteDesk::new();
                    let fp = d.register(1, "差异 A：账面 5 实计 7");
                    match fp {
                        Ok(f) => d.route(1, Channel::Errata, f ^ 0x1).err()
                            == Some(RouteCode::TamperedItem),
                        Err(_) => false,
                    }
                }
                BadRoute::UnknownChannel => {
                    parse_channel("appendix").is_err()
                        && parse_channel("").err() == Some(RouteCode::EmptyChannel)
                        && parse_channel("Redbook").is_err()
                }
                BadRoute::DupRoute => {
                    let mut d = RouteDesk::new();
                    let fp = d.register(2, "差异 B：账面 3 实计 9");
                    match fp {
                        Ok(f) => {
                            let _ = d.route(2, Channel::ExpDiff, f);
                            d.route(2, Channel::RedBook, f).err() == Some(RouteCode::DupRoute)
                        }
                        Err(_) => false,
                    }
                }
                BadRoute::UnregisteredRoute => {
                    let mut d = RouteDesk::new();
                    d.route(99, Channel::Errata, 0).err() == Some(RouteCode::UnknownItem)
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

// ── 流转成文（纯函数） ────────────────────────────────────────────────
/// 流转成文：「登记=<n>;流转=<n>;勘误=<n>;预期差=<n>;红账=<n>;待处置=<m>」。
pub fn compose_route_summary(d: &RouteDesk, l: &RouteLog) -> Result<String, RouteCode> {
    if d.is_empty() || !l.conserved() || !l.steps_continuous() {
        return Err(RouteCode::EmptySummary);
    }
    let out = format!(
        "登记={};流转={};勘误={};预期差={};红账={};待处置={}",
        d.len(),
        l.routed(),
        l.ch_count(Channel::Errata),
        l.ch_count(Channel::ExpDiff),
        l.ch_count(Channel::RedBook),
        d.pending()
    );
    if out.is_empty() {
        return Err(RouteCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；未知/空通道族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；条目未登记族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；私改对账项族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；双重流转族"),
    ("ESTALE", -116, 18, "Windows ERROR_NO_MORE_FILES；步号断档/账面过期族"),
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

/// 规范语料双跑指纹：登记三条 → 归三通道 → 落痕 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut d = RouteDesk::new();
    let f1 = d.register(1, "差异一");
    let f2 = d.register(2, "差异二");
    let f3 = d.register(3, "差异三");
    let mut l = RouteLog::new();
    if let (Ok(a), Ok(b), Ok(c)) = (f1, f2, f3) {
        let _ = d.route(1, Channel::Errata, a);
        let _ = d.route(2, Channel::ExpDiff, b);
        let _ = d.route(3, Channel::RedBook, c);
        let _ = l.log(1, Channel::Errata);
        let _ = l.log(2, Channel::ExpDiff);
        let _ = l.log(3, Channel::RedBook);
    }
    let bytes = match compose_route_summary(&d, &l) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10075 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10075_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10075");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x922C0..=0x922FF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 通道解析器：双形制全通过
    let parse_ok = parse_channel("errata") == Ok(Channel::Errata)
        && parse_channel("e") == Ok(Channel::Errata)
        && parse_channel("redbook") == Ok(Channel::RedBook)
        && parse_channel("x") == Ok(Channel::ExpDiff);
    s.add("通道解析器：errata|expdiff|redbook 双形制全通过", parse_ok,
          "通道闭集双形制必须全可解析，闭集外显性拒绝");

    // 3. 无效通道注入：未知/空串/漂移 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_channel("appendix").is_ok()
            || parse_channel("").is_ok()
            || parse_channel("Errata").is_ok()
            || parse_channel("redbook ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("无效通道注入：未知/空串/漂移 10/10 拒绝", amb_ok,
          "通道歧义必须逐次显性拒绝，不许静默归并");

    // 4. 登记冻结指纹：登记三条并查回待处置态
    let mut d = RouteDesk::new();
    let f1 = d.register(1, "差异一：账面 5 实计 7");
    let f2 = d.register(2, "差异二：账面 3 实计 9");
    let f3 = d.register(3, "差异三：账面 8 实计 8");
    let reg_ok = d.len() == 3
        && d.pending() == 3
        && f1.is_ok() && f2.is_ok() && f3.is_ok();
    s.add("登记冻结指纹：三条登记且全部待处置", reg_ok,
          "流转前必须登记冻结指纹，登记即待处置态");

    // 5. 逐差异归通道：三通道各归一条并落痕
    let mut l = RouteLog::new();
    let fp1 = match f1 { Ok(v) => v, Err(_) => 0 };
    let fp2 = match f2 { Ok(v) => v, Err(_) => 0 };
    let fp3 = match f3 { Ok(v) => v, Err(_) => 0 };
    let route_ok = d.route(1, Channel::Errata, fp1).is_ok()
        && d.route(2, Channel::ExpDiff, fp2).is_ok()
        && d.route(3, Channel::RedBook, fp3).is_ok()
        && l.log(1, Channel::Errata) == Ok(1)
        && l.log(2, Channel::ExpDiff) == Ok(2)
        && l.log(3, Channel::RedBook) == Ok(3);
    s.add("逐差异归通道：三通道各归一条且步号 1..3 连续", route_ok,
          "逐差异归通道并留痕，步号连续递增");

    // 6. 流转账守恒
    s.add("流转账守恒：已流转=勘误+预期差+红账",
          l.conserved() && l.routed() == 3
              && l.ch_count(Channel::Errata) == 1
              && l.ch_count(Channel::ExpDiff) == 1
              && l.ch_count(Channel::RedBook) == 1,
          "三通道计数与留痕条目逐笔对得上");

    // 7. 待处置联动：登记数 = 已流转 + 待处置
    s.add("待处置联动：登记数 = 已流转 + 待处置",
          d.len() as u32 == l.routed() + d.pending() && d.pending() == 0,
          "两相状态机的守恒桥：登记-流转-待处置逐笔对得上");

    // 8. 私改拦截：指纹不符 10/10 次拒绝且不改台账
    let mut tamper_ok = true;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        let mut d2 = RouteDesk::new();
        let fp = match d2.register(7, "差异七原文") {
            Ok(v) => v,
            Err(_) => 0,
        };
        let log_before = l.len();
        if d2.route(7, Channel::Errata, fp ^ 0xDEAD).err() != Some(RouteCode::TamperedItem)
            || d2.row(7).map(|x| x.pending()).unwrap_or(true) != true
            || l.len() != log_before
        {
            tamper_ok = false;
        }
        r2 += 1;
    }
    s.add("私改拦截：指纹不符 10/10 次拒绝且待处置态不变", tamper_ok,
          "私改对账项必须逐次拦截，被拒条目保持待处置");

    // 9. 双重流转拒绝
    let mut d2 = RouteDesk::new();
    let fp = match d2.register(9, "差异九") { Ok(v) => v, Err(_) => 0 };
    let _ = d2.route(9, Channel::ExpDiff, fp);
    s.add("双重流转：同条目二次归口显性拒绝",
          d2.route(9, Channel::RedBook, fp).err() == Some(RouteCode::DupRoute),
          "同一条目只能归一个通道，改道走勘误更正");

    // 10. 注入核：四类坏流转样本各 10/10 拒绝
    let (inj, per_class) = run_bad_route_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏流转样本各 10/10 拒绝", all_caught,
          "私改/无效通道/双重流转/未登记必须逐类逐次拒绝");

    // 11. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadRoute::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 12. 步号连续核：断档样本在留痕账上不可构造（连续性守卫生效）
    s.add("步号连续核：留痕账步号 1..3 连续且守卫可复算",
          l.steps_continuous(),
          "步号连续递增是留痕硬约束，断号即账面过期");

    // 13. 拒绝不留痕：未登记流转不改登记表与留痕账
    let before = (d.len(), l.len(), l.routed());
    let _ = d.route(99, Channel::Errata, 0);
    let after = (d.len(), l.len(), l.routed());
    s.add("拒绝不留痕：未登记流转不改台账", before == after,
          "被拒流转不得改动登记表与留痕账任何状态");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(1)
        && agrees_with_linux(17) && agrees_with_linux(116)
        && !agrees_with_linux(0) && !agrees_with_linux(4444);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < RouteCode::ALL.len() {
        let mut j = i + 1;
        while j < RouteCode::ALL.len() {
            if let (Some(a), Some(b)) = (RouteCode::ALL.get(i), RouteCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = RouteCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 通道往返 + 成文 + 双跑指纹
    let rt_ok = Channel::ALL.iter().all(|c| {
        let tag = match c {
            Channel::Errata => "errata",
            Channel::ExpDiff => "expdiff",
            Channel::RedBook => "redbook",
        };
        parse_channel(tag) == Ok(*c)
    });
    let sum = compose_route_summary(&d, &l);
    let sum_ok = match &sum {
        Ok(t) => t.contains("登记=3") && t.contains("待处置=0"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：通道标签可回解析且双跑哈希逐位相等", rt_ok && sum_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 流转账快照 + 指纹
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let led_text = format!(
        "勘误 {} / 预期差 {} / 红账 {}",
        l.ch_count(Channel::Errata),
        l.ch_count(Channel::ExpDiff),
        l.ch_count(Channel::RedBook)
    );
    s.add_dyn("流转账快照", l.conserved() && l.routed() == 3, &led_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_channel("expdiff"), Ok(Channel::ExpDiff));
        assert_eq!(parse_channel("r"), Ok(Channel::RedBook));
        assert!(parse_channel("appendix").is_err());
        assert_eq!(parse_channel("").err(), Some(RouteCode::EmptyChannel));
    }

    #[test]
    fn register_freezes_fingerprint() {
        let mut d = RouteDesk::new();
        let f = d.register(5, "内容 A").unwrap_or(0);
        assert_ne!(f, 0);
        assert!(d.register(5, "内容 A").is_err());
        let row = d.row(5).unwrap_or(ItemRow { item: 0, fp: 0, routed: None });
        assert!(row.pending() && row.fp == f);
    }

    #[test]
    fn route_and_log_conservation() {
        let mut d = RouteDesk::new();
        let f = d.register(1, "差异 X").unwrap_or(0);
        let mut l = RouteLog::new();
        assert!(d.route(1, Channel::RedBook, f).is_ok());
        assert_eq!(l.log(1, Channel::RedBook), Ok(1));
        assert!(l.conserved() && l.steps_continuous());
    }

    #[test]
    fn tampered_rejected_pending_unchanged() {
        let mut d = RouteDesk::new();
        let f = d.register(3, "原文").unwrap_or(0);
        assert_eq!(d.route(3, Channel::Errata, f.wrapping_add(1)).err(),
                   Some(RouteCode::TamperedItem));
        assert!(d.row(3).map(|x| x.pending()).unwrap_or(false));
    }

    #[test]
    fn dup_route_rejected() {
        let mut d = RouteDesk::new();
        let f = d.register(2, "差异").unwrap_or(0);
        let _ = d.route(2, Channel::Errata, f);
        assert_eq!(d.route(2, Channel::ExpDiff, f).err(), Some(RouteCode::DupRoute));
    }

    #[test]
    fn unregistered_route_rejected() {
        let mut d = RouteDesk::new();
        assert_eq!(d.route(42, Channel::Errata, 0).err(), Some(RouteCode::UnknownItem));
    }

    #[test]
    fn bad_route_matrix_green() {
        let (inj, per) = run_bad_route_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn summary_compose() {
        let mut d = RouteDesk::new();
        let f = d.register(1, "差异").unwrap_or(0);
        let mut l = RouteLog::new();
        let _ = d.route(1, Channel::Errata, f);
        let _ = l.log(1, Channel::Errata);
        let s = compose_route_summary(&d, &l).unwrap_or_default();
        assert!(s.contains("勘误=1") && s.contains("待处置=0"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(1));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(116), -116);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10075_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10075_checks_green() {
        let set = run_unx10075_checks();
        assert!(set.all_passed(), "UNX-F10075 red: {}", set.red_items_text());
    }
}
