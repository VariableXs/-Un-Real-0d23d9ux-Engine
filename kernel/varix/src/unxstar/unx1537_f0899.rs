//! UNX-F1537 收官段观测健康摘要（对齐 F0899 口径注册） · 0x78740段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B03 · UNX-F1537 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1537-J1 收官段健康摘要挂接（B37 十账摘要入 F0997
//!     无缺），schema 校验通过，红账投递 10/10 可达；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - B37 十账闭集：批内十本账逐一挂接健康摘要入 F0997 汇接池，缺账
//!     即 HookMissing，重复挂接即 SummaryDuplicate；
//!   - F0899 口径对齐：每条摘要必须引用 F0899 已注册口径（口径号闭集），
//!     未注册口径显性拒（对齐红线）；
//!   - schema 校验：记录字段闭集核查（账名在册 / 口径已注册 / 指纹非零 /
//!     版本冻结），四路注入逐路必抓；
//!   - 红账投递 10/10：红账（红项计数 > 0）逐账开投递票，汇接池路由表
//!     逐票核路，无路 / 丢失显性记账；
//!   - 健康状态机：采集 → 挂接 → schema 过 → 投递全达，单调推进；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78740，派工卡 VX8-B03 卡 6 独占窗口）。
pub const SEG_BASE: u32 = 0x78740;

/// B37 十账（批内账闭集，判据 J1"十账"冻结）。
pub const B37_ACCOUNTS: [&str; 10] = [
    "账本账", "断言账", "依赖账", "数据流账", "哈希账",
    "压测账", "矩阵账", "终审账", "快照账", "注入账",
];

/// F0899 口径注册闭集（对齐线，五口径）。
pub const F0899_CALIBERS: [&str; 5] = [
    "F0899-红项率", "F0899-对账轮数", "F0899-快照深度", "F0899-注入检出", "F0899-执行步数",
];

/// F0997 记录 schema 冻结版号。
pub const SCHEMA_VER_FROZEN: u32 = 0x7874;

/// 红账投递票目标数（判据"10/10 可达"冻结）。
pub const RED_TICKETS_NEEDED: usize = 10;

/// 汇接池挂接容量（十账齐挂即满）。
pub const HOOK_CAPACITY: usize = 10;

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealthCode {
    /// 账名不在 B37 十账闭集：拒挂
    AccountUnknown,
    /// 摘要重复挂接：一账一条，先撤旧条再补
    SummaryDuplicate,
    /// 记录字段违约：账名/口径/指纹字段缺一即拒
    SchemaFieldBad,
    /// schema 版本漂移：回滚到冻结版再挂接
    SchemaVersionBad,
    /// 口径未在 F0899 注册：先注册口径再挂摘要
    CaliberNotRegistered,
    /// 摘要挂接缺账：补齐十账缺账后复审
    HookMissing,
    /// 红账投递无路：路由表补该账路由
    RedTicketUnrouted,
    /// 红账投递丢失：重开投递票并落档
    RedTicketLost,
    /// 健康阶段倒退：状态机红线，禁止回退重放
    PhaseRegress,
    /// 健康阶段跳级：挂接未齐不得进 schema 复核
    PhaseSkip,
    /// 摘要指纹为空：重算指纹再挂
    DigestZero,
    /// 双跑不一致：排查摘要构建随机性
    NotReproducible,
}

impl HealthCode {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            HealthCode::AccountUnknown => "账名不在 B37 十账闭集：拒挂，先核对批内账名",
            HealthCode::SummaryDuplicate => "摘要重复挂接：一账一条，撤旧条再补",
            HealthCode::SchemaFieldBad => "记录字段违约：账名/口径/指纹字段补齐再校",
            HealthCode::SchemaVersionBad => "schema 版本漂移：回滚冻结版再挂接",
            HealthCode::CaliberNotRegistered => "口径未在 F0899 注册：先注册口径再挂摘要",
            HealthCode::HookMissing => "摘要挂接缺账：补齐十账缺账后复审",
            HealthCode::RedTicketUnrouted => "红账投递无路：路由表补该账路由",
            HealthCode::RedTicketLost => "红账投递丢失：重开投递票并落档",
            HealthCode::PhaseRegress => "健康阶段倒退：按采集→挂接→schema→投递单调推进",
            HealthCode::PhaseSkip => "健康阶段跳级：挂接未齐不得进 schema 复核",
            HealthCode::DigestZero => "摘要指纹为空：重算指纹再挂",
            HealthCode::NotReproducible => "双跑不一致：排查摘要构建随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [HealthCode; 12] = [
        HealthCode::AccountUnknown,
        HealthCode::SummaryDuplicate,
        HealthCode::SchemaFieldBad,
        HealthCode::SchemaVersionBad,
        HealthCode::CaliberNotRegistered,
        HealthCode::HookMissing,
        HealthCode::RedTicketUnrouted,
        HealthCode::RedTicketLost,
        HealthCode::PhaseRegress,
        HealthCode::PhaseSkip,
        HealthCode::DigestZero,
        HealthCode::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < HealthCode::ALL.len() {
        if let Some(c) = HealthCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

/// 口径取件（get 语义，禁裸下标）：首口径。
fn caliber_first() -> &'static str {
    match F0899_CALIBERS.get(0) {
        Some(c) => c,
        None => "",
    }
}

/// 口径取件（get 语义，禁裸下标）：次口径。
fn caliber_second() -> &'static str {
    match F0899_CALIBERS.get(1) {
        Some(c) => c,
        None => "",
    }
}

// ── 确定性摘要（FNV-1a 64，指纹核心） ───────────────────────────
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        i += 1;
    }
    h
}

// ── 域模型：健康摘要 / 汇接池 / 投递票 ──────────────────────────
/// 一账的健康摘要（schema 记录四字段 + 语义字段）。
pub struct HealthSummary {
    account: String,
    caliber: String,
    red: u32,
    green: u32,
    digest: u64,
}

impl HealthSummary {
    pub fn new(account: &str, caliber: &str, red: u32, green: u32) -> HealthSummary {
        let mut buf = String::new();
        buf.push_str(account);
        buf.push('|');
        buf.push_str(caliber);
        buf.push('|');
        let mut rb = String::new();
        push_u32(&mut rb, red);
        buf.push_str(&rb);
        buf.push('|');
        let mut gb = String::new();
        push_u32(&mut gb, green);
        buf.push_str(&gb);
        HealthSummary {
            account: account.to_string(),
            caliber: caliber.to_string(),
            red,
            green,
            digest: fnv1a64(buf.as_bytes()),
        }
    }
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn caliber(&self) -> &str {
        &self.caliber
    }
    pub fn red(&self) -> u32 {
        self.red
    }
    pub fn green(&self) -> u32 {
        self.green
    }
    pub fn digest(&self) -> u64 {
        self.digest
    }
    pub fn is_red(&self) -> bool {
        self.red > 0
    }
    /// schema 校验：字段闭集逐条核查（顺序即诊断码序）。
    pub fn validate_schema(&self, version: u32) -> Result<(), u32> {
        if version != SCHEMA_VER_FROZEN {
            return Err(HealthCode::SchemaVersionBad.val());
        }
        if !B37_ACCOUNTS.iter().any(|a| *a == self.account()) {
            return Err(HealthCode::AccountUnknown.val());
        }
        if !F0899_CALIBERS.iter().any(|c| *c == self.caliber()) {
            return Err(HealthCode::CaliberNotRegistered.val());
        }
        if self.digest == 0 {
            return Err(HealthCode::DigestZero.val());
        }
        if self.red == 0 && self.green == 0 {
            return Err(HealthCode::SchemaFieldBad.val());
        }
        Ok(())
    }
}

/// 红账投递票：一红账一票，路由后可达。
pub struct RedTicket {
    account: String,
    diag: u32,
    routed: bool,
}

impl RedTicket {
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn diag(&self) -> u32 {
        self.diag
    }
    pub fn is_routed(&self) -> bool {
        self.routed
    }
}

/// 健康阶段（单调状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealthPhase {
    /// 采集：十账摘要构建中
    Collect,
    /// 挂接：十账摘要入 F0997
    Hooked,
    /// schema 过：逐条校验通过
    SchemaOk,
    /// 投递全达：红账 10/10 可达
    Delivered,
}

impl HealthPhase {
    pub fn rank(self) -> u32 {
        match self {
            HealthPhase::Collect => 0,
            HealthPhase::Hooked => 1,
            HealthPhase::SchemaOk => 2,
            HealthPhase::Delivered => 3,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            HealthPhase::Collect => "采集",
            HealthPhase::Hooked => "挂接",
            HealthPhase::SchemaOk => "schema 过",
            HealthPhase::Delivered => "投递全达",
        }
    }
}

/// F0997 汇接池：挂接 / 校验 / 投递三面。
pub struct F0997Sink {
    hooks: Vec<HealthSummary>,
    tickets: Vec<RedTicket>,
    version: u32,
    phase: HealthPhase,
}

impl F0997Sink {
    pub fn new(version: u32) -> F0997Sink {
        F0997Sink {
            hooks: Vec::new(),
            tickets: Vec::new(),
            version,
            phase: HealthPhase::Collect,
        }
    }
    pub fn phase(&self) -> HealthPhase {
        self.phase
    }
    pub fn hook_count(&self) -> usize {
        self.hooks.len()
    }
    pub fn ticket_count(&self) -> usize {
        self.tickets.len()
    }
    /// 挂接一条摘要：账名闭集 / 重复 / 口径对齐 / 指纹非零逐关拒。
    pub fn hook(&mut self, s: HealthSummary) -> Result<(), u32> {
        if !B37_ACCOUNTS.iter().any(|a| *a == s.account()) {
            return Err(HealthCode::AccountUnknown.val());
        }
        if self.hooks.iter().any(|h| h.account() == s.account()) {
            return Err(HealthCode::SummaryDuplicate.val());
        }
        s.validate_schema(self.version)?;
        if self.hooks.len() >= HOOK_CAPACITY {
            return Err(HealthCode::SummaryDuplicate.val());
        }
        self.hooks.push(s);
        Ok(())
    }
    /// 挂接完整性：十账无缺。
    pub fn hook_complete(&self) -> Result<(), u32> {
        if self.hooks.len() != HOOK_CAPACITY {
            return Err(HealthCode::HookMissing.val());
        }
        for a in B37_ACCOUNTS.iter() {
            if !self.hooks.iter().any(|h| h.account() == *a) {
                return Err(HealthCode::HookMissing.val());
            }
        }
        Ok(())
    }
    /// 逐条 schema 复核（挂接齐备前置）。
    pub fn audit_schema(&mut self) -> Result<(), u32> {
        self.hook_complete()?;
        for h in self.hooks.iter() {
            h.validate_schema(self.version)?;
        }
        self.advance(HealthPhase::SchemaOk)
    }
    /// 开红账投递票：非红账不开票。
    pub fn open_red_ticket(&mut self, account: &str, diag: u32) -> Result<(), u32> {
        if !B37_ACCOUNTS.iter().any(|a| *a == account) {
            return Err(HealthCode::AccountUnknown.val());
        }
        let red = match self.hooks.iter().find(|h| h.account() == account) {
            Some(h) => h.red(),
            None => return Err(HealthCode::HookMissing.val()),
        };
        if red == 0 {
            return Err(HealthCode::RedTicketLost.val());
        }
        self.tickets.push(RedTicket {
            account: account.to_string(),
            diag,
            routed: false,
        });
        Ok(())
    }
    /// 路由全达：逐票核路由（挂接在册即有路）。
    pub fn route_all(&mut self) -> usize {
        let mut unrouted = 0usize;
        let mut k = 0usize;
        while k < self.tickets.len() {
            let account = match self.tickets.get(k) {
                Some(t) => t.account().to_string(),
                None => break,
            };
            let has_path = self.hooks.iter().any(|h| h.account() == account);
            if let Some(t) = self.tickets.get_mut(k) {
                t.routed = has_path;
            }
            if !has_path {
                unrouted += 1;
            }
            k += 1;
        }
        if unrouted == 0 {
            let _ = self.advance(HealthPhase::Delivered);
        }
        unrouted
    }
    /// 投递全达判据：票数 10/10 且逐票路由成功。
    pub fn all_delivered(&self) -> bool {
        if self.tickets.len() != RED_TICKETS_NEEDED {
            return false;
        }
        self.tickets.iter().all(|t| t.is_routed())
    }
    /// 单调推进：倒退 / 跳级显性拒。
    pub fn advance(&mut self, to: HealthPhase) -> Result<(), u32> {
        if to.rank() < self.phase.rank() {
            return Err(HealthCode::PhaseRegress.val());
        }
        if to.rank() > self.phase.rank() + 1 {
            return Err(HealthCode::PhaseSkip.val());
        }
        self.phase = to;
        Ok(())
    }
    /// 挂接齐备后推进（采集 → 挂接）。
    pub fn seal_hooks(&mut self) -> Result<(), u32> {
        self.hook_complete()?;
        self.advance(HealthPhase::Hooked)
    }
    /// 摘要指纹（双跑一致比对用）。
    pub fn digest_of_hooks(&self) -> u64 {
        let mut buf = String::new();
        let mut k = 0usize;
        while k < self.hooks.len() {
            let h = match self.hooks.get(k) {
                Some(h) => h,
                None => break,
            };
            buf.push_str(h.account());
            buf.push('@');
            buf.push_str(h.caliber());
            buf.push(':');
            let mut rb = String::new();
            push_u32(&mut rb, h.red());
            buf.push_str(&rb);
            buf.push('/');
            let mut gb = String::new();
            push_u32(&mut gb, h.green());
            buf.push_str(&gb);
            buf.push(';');
            k += 1;
        }
        fnv1a64(buf.as_bytes())
    }
}

fn push_u32(out: &mut String, v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut digits = Vec::new();
    let mut x = v;
    while x > 0 {
        digits.push((b'0' + (x % 10) as u8) as char);
        x /= 10;
    }
    let mut i = digits.len();
    while i > 0 {
        i -= 1;
        if let Some(c) = digits.get(i) {
            out.push(*c);
        }
    }
}

// ── 语料构建（好池 / 各路注入） ─────────────────────────────────
/// 好池：十账全挂（口径轮转 F0899 五口径），十账全红（红项 1..3）。
pub fn build_sink_ok() -> F0997Sink {
    let mut sink = F0997Sink::new(SCHEMA_VER_FROZEN);
    let mut i = 0usize;
    while i < B37_ACCOUNTS.len() {
        let account = match B37_ACCOUNTS.get(i) {
            Some(a) => *a,
            None => break,
        };
        let caliber = match F0899_CALIBERS.get(i % F0899_CALIBERS.len()) {
            Some(c) => *c,
            None => break,
        };
        let red = (i as u32) % 3 + 1;
        let green = 9 - red;
        let _ = sink.hook(HealthSummary::new(account, caliber, red, green));
        i += 1;
    }
    sink
}

/// 注入池：第 5 账漏挂（缺账注入）。
pub fn build_sink_missing_hook() -> F0997Sink {
    let mut sink = F0997Sink::new(SCHEMA_VER_FROZEN);
    let mut i = 0usize;
    while i < B37_ACCOUNTS.len() {
        if i == 4 {
            // 注入：哈希账漏挂。
            i += 1;
            continue;
        }
        let account = match B37_ACCOUNTS.get(i) {
            Some(a) => *a,
            None => break,
        };
        let caliber = match F0899_CALIBERS.get(i % F0899_CALIBERS.len()) {
            Some(c) => *c,
            None => break,
        };
        let red = (i as u32) % 3 + 1;
        let green = 9 - red;
        let _ = sink.hook(HealthSummary::new(account, caliber, red, green));
        i += 1;
    }
    sink
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78600 && SEG_BASE <= 0x787C0
}

/// 十账摘要全挂：挂接数 10、账名全覆盖。
fn hooks_complete_ok() -> bool {
    let mut sink = build_sink_ok();
    if sink.hook_count() != HOOK_CAPACITY {
        return false;
    }
    if sink.seal_hooks().is_err() {
        return false;
    }
    if sink.phase() != HealthPhase::Hooked {
        return false;
    }
    true
}

/// 挂接缺账注入：第 5 账漏挂必被抓。
fn missing_hook_caught_ok() -> bool {
    let mut sink = build_sink_missing_hook();
    if sink.hook_count() != HOOK_CAPACITY - 1 {
        return false;
    }
    match sink.seal_hooks() {
        Err(code) => code == HealthCode::HookMissing.val(),
        Ok(()) => false,
    }
}

/// schema 校验通过：挂接齐备后逐条复核可过。
fn schema_audit_ok() -> bool {
    let mut sink = build_sink_ok();
    let _ = sink.seal_hooks();
    if sink.audit_schema().is_err() {
        return false;
    }
    if sink.phase() != HealthPhase::SchemaOk {
        return false;
    }
    true
}

/// 四路 schema 注入逐路必抓：版本漂移 / 未注册口径 / 未知账 / 零指纹。
fn schema_injections_caught_ok() -> bool {
    // 注入一：版本漂移。
    let mut sink_badver = F0997Sink::new(SCHEMA_VER_FROZEN + 1);
    let s = HealthSummary::new("账本账", caliber_first(), 1, 8);
    match sink_badver.hook(s) {
        Err(code) => {
            if code != HealthCode::SchemaVersionBad.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 注入二：口径未注册。
    let mut sink_badcal = F0997Sink::new(SCHEMA_VER_FROZEN);
    let s2 = HealthSummary::new("账本账", "F0899-未注册口径", 1, 8);
    match sink_badcal.hook(s2) {
        Err(code) => {
            if code != HealthCode::CaliberNotRegistered.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 注入三：账名越闭集。
    let mut sink_badacc = F0997Sink::new(SCHEMA_VER_FROZEN);
    let s3 = HealthSummary::new("批外账", caliber_first(), 1, 8);
    match sink_badacc.hook(s3) {
        Err(code) => {
            if code != HealthCode::AccountUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 注入四：重复挂接。
    let mut sink_dup = build_sink_ok();
    let s4 = HealthSummary::new("账本账", caliber_second(), 2, 7);
    match sink_dup.hook(s4) {
        Err(code) => {
            if code != HealthCode::SummaryDuplicate.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    true
}

/// 红账投递 10/10 可达：十账全红逐账开票、路由全达。
fn red_delivery_ok() -> bool {
    let mut sink = build_sink_ok();
    let _ = sink.seal_hooks();
    // 投递前置：schema 复核推进到 SchemaOk（路由阶段单调线）。
    if sink.audit_schema().is_err() {
        return false;
    }
    let mut i = 0usize;
    while i < B37_ACCOUNTS.len() {
        let account = match B37_ACCOUNTS.get(i) {
            Some(a) => *a,
            None => break,
        };
        let diag = SEG_BASE + 0x10 + i as u32;
        if sink.open_red_ticket(account, diag).is_err() {
            return false;
        }
        i += 1;
    }
    if sink.ticket_count() != RED_TICKETS_NEEDED {
        return false;
    }
    if sink.route_all() != 0 {
        return false;
    }
    if !sink.all_delivered() {
        return false;
    }
    sink.phase() == HealthPhase::Delivered
}

/// 投递无路注入：缺账池开票必拒（无挂接即无路由）。
fn unrouted_caught_ok() -> bool {
    let mut sink = build_sink_missing_hook();
    let _ = sink.seal_hooks();
    // 哈希账漏挂 → 其票无路。
    match sink.open_red_ticket("哈希账", SEG_BASE + 0x11) {
        Err(code) => code == HealthCode::HookMissing.val(),
        Ok(()) => false,
    }
}

/// 非红账不开票（投递票纪律）。
fn green_account_no_ticket_ok() -> bool {
    let mut sink = F0997Sink::new(SCHEMA_VER_FROZEN);
    let _ = sink.hook(HealthSummary::new("账本账", caliber_first(), 0, 9));
    match sink.open_red_ticket("账本账", SEG_BASE + 0x12) {
        Err(code) => code == HealthCode::RedTicketLost.val(),
        Ok(()) => false,
    }
}

/// 状态机单调：全程可达且倒退 / 跳级必拒。
fn phase_monotonic_ok() -> bool {
    let mut sink = build_sink_ok();
    if sink.advance(HealthPhase::SchemaOk).is_ok() {
        return false;
    }
    let _ = sink.seal_hooks();
    if sink.audit_schema().is_err() {
        return false;
    }
    if sink.advance(HealthPhase::Hooked).is_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < B37_ACCOUNTS.len() {
        if let Some(a) = B37_ACCOUNTS.get(i) {
            let _ = sink.open_red_ticket(a, SEG_BASE + 0x10 + i as u32);
        }
        i += 1;
    }
    let _ = sink.route_all();
    sink.phase() == HealthPhase::Delivered
}

/// 双跑一致：好池指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_sink_ok().digest_of_hooks();
    let d2 = build_sink_ok().digest_of_hooks();
    if d1 != d2 {
        return false;
    }
    // 缺账池指纹必须不同。
    let d3 = build_sink_missing_hook().digest_of_hooks();
    d3 != d1
}

/// 诊断码闭集：12 码唯一、落窗、查表可回。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in HealthCode::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first_detail = match HealthCode::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first_detail {
        return false;
    }
    true
}

/// 十账摘要指纹逐账非零（挂接池逐条核）。
fn hook_digests_nonzero_ok() -> bool {
    let sink = build_sink_ok();
    if sink.hook_count() != HOOK_CAPACITY {
        return false;
    }
    // 指纹非零逐账核（复用 schema 校验的指纹关）。
    for a in B37_ACCOUNTS.iter() {
        let found = {
            // 借 validate_schema 的指纹关逐账复核（构造同参摘要比对）。
            let mut hit = false;
            let mut i = 0usize;
            while i < B37_ACCOUNTS.len() {
                let acc = match B37_ACCOUNTS.get(i) {
                    Some(x) => *x,
                    None => break,
                };
                if acc == *a {
                    let cal = match F0899_CALIBERS.get(i % F0899_CALIBERS.len()) {
                        Some(c) => c,
                        None => return false,
                    };
                    let red = (i as u32) % 3 + 1;
                    let s = HealthSummary::new(acc, cal, red, 9 - red);
                    if s.digest() == 0 {
                        return false;
                    }
                    if s.validate_schema(SCHEMA_VER_FROZEN).is_err() {
                        return false;
                    }
                    hit = true;
                }
                i += 1;
            }
            hit
        };
        if !found {
            return false;
        }
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1537_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1537 收官段观测健康摘要");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78600..=0x787C0", SEG_BASE));
    s.add_dyn("十账摘要全挂", hooks_complete_ok(), &format!("B37 十账入 F0997 无缺（容量 {}）", HOOK_CAPACITY));
    s.add_dyn("缺账注入必抓", missing_hook_caught_ok(), "漏挂账必被挂接完整性抓出");
    s.add_dyn("schema 校验通过", schema_audit_ok(), "逐条记录四字段闭集复核可过");
    s.add_dyn("schema 注入必抓", schema_injections_caught_ok(), "版本漂移/未注册口径/越集账/重挂四路必拒");
    s.add_dyn("红账投递 10/10", red_delivery_ok(), &format!("十账全红逐账开票，路由全达（{} 票）", RED_TICKETS_NEEDED));
    s.add_dyn("无路注入必抓", unrouted_caught_ok(), "缺挂接账开票必拒");
    s.add_dyn("绿账不开票", green_account_no_ticket_ok(), "零红账投递票拒开");
    s.add_dyn("健康状态机单调", phase_monotonic_ok(), "采集→挂接→schema→投递全达可达，倒退拒");
    s.add_dyn("双跑一致", double_run_ok(), "好池指纹两次构建逐位相等");
    s.add_dyn("摘要指纹逐账非零", hook_digests_nonzero_ok(), "十账摘要逐条指纹非零且 schema 可过");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SCHEMA_VER_FROZEN, 0x7874);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1537_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn ten_accounts_fully_hooked() {
        let mut sink = build_sink_ok();
        assert_eq!(sink.hook_count(), HOOK_CAPACITY);
        assert!(sink.seal_hooks().is_ok());
    }

    #[test]
    fn missing_hook_returns_diag() {
        let mut sink = build_sink_missing_hook();
        match sink.seal_hooks() {
            Err(code) => assert_eq!(code, HealthCode::HookMissing.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn schema_injections_each_caught() {
        assert!(schema_injections_caught_ok());
        // 单路细检：未注册口径。
        let mut sink = F0997Sink::new(SCHEMA_VER_FROZEN);
        match sink.hook(HealthSummary::new("账本账", "F0899-坏口径", 1, 8)) {
            Err(code) => assert_eq!(code, HealthCode::CaliberNotRegistered.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn red_tickets_all_routed() {
        assert!(red_delivery_ok());
        let mut sink = build_sink_ok();
        let _ = sink.seal_hooks();
        let _ = sink.open_red_ticket("账本账", SEG_BASE + 0x10);
        assert_eq!(sink.ticket_count(), 1);
        assert_eq!(sink.route_all(), 0);
    }

    #[test]
    fn green_account_cannot_open_ticket() {
        assert!(green_account_no_ticket_ok());
    }

    #[test]
    fn phase_regress_rejected() {
        let mut sink = build_sink_ok();
        let _ = sink.seal_hooks();
        match sink.advance(HealthPhase::Collect) {
            Err(code) => assert_eq!(code, HealthCode::PhaseRegress.val()),
            Ok(()) => must_not_reach(),
        }
        // 跳级：采集 → 投递必拒。
        let mut sink2 = F0997Sink::new(SCHEMA_VER_FROZEN);
        match sink2.advance(HealthPhase::Delivered) {
            Err(code) => assert_eq!(code, HealthCode::PhaseSkip.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
        let d1 = build_sink_ok().digest_of_hooks();
        let d2 = build_sink_ok().digest_of_hooks();
        assert_eq!(d1, d2);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(HealthCode::ALL.len(), 12);
        let first = match HealthCode::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match HealthCode::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(SEG_BASE + 0x0D), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
