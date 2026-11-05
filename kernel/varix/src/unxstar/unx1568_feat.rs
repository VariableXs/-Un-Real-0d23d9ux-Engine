//! UNX-F1568 联签记录总账（逐域逐件签收留痕） · 0x78D00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-1104 B06 卡 5 · UNX-F1568 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1568-J1 联签总账齐备（逐域逐件签收记录：
//!     域/件/时刻/结论四列），记录与总表对平 100%，账哈希落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 总表闭集：4 域 × 3 件 = 12 条签收位逐位冻结（域名 / 件名 /
//!     基准时刻），缺位即对平拒；
//!   - 记录四列：域 / 件 / 时刻 / 结论逐列登记，时刻须 ≥ 基准时刻
//!     （单调不倒流），结论闭集（签收 / 留痕），域件组合须命中总表；
//!   - 对平 100%：12 条记录与 12 条总表逐条对平（域件时刻三列齐
//!     对），缺一条即对平拒；
//!   - 账哈希落档：12 记录序贯合成 FNV-1a 账指纹 + 14 行总账落档
//!     双跑逐位一致；
//!   - 时刻倒流注入 / 陌生域注入 / 重复签收注入三路必抓；
//!   - 状态机单调：登记→对平→落档，倒退跳级显性拒；
//!   - LCG 抽验复验，种子写死 = SEG_BASE；零浮点 / IO / 墙钟 /
//!     unwrap / panic；整数运算；中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78D00，派工卡 VX8-1104 B06 卡 5 独占窗口）。
pub const SEG_BASE: u32 = 0x78D00;

/// 签收域数（冻结）。
pub const DOMAIN_COUNT: usize = 4;

/// 每域签收件数（冻结）。
pub const ITEMS_PER_DOMAIN: usize = 3;

/// 总账签收位总数（4 域 × 3 件 = 12）。
pub const LEDGER_SLOTS: usize = 12;

/// 总账落档行数（头 + 12 记录行 + 尾，冻结）。
pub const LEDGER_ROWS: usize = 14;

// ── 签收域闭集 ──────────────────────────────────────────────────
pub const DOMAINS: [&str; 4] = ["中断域", "计时域", "内存域", "调试域"];

// ── 每域签收件闭集（3 件） ──────────────────────────────────────
pub const ITEMS: [&str; 3] = ["联签件", "终验件", "回执件"];

// ── 总表冻结：逐签收位基准时刻（单调不倒流的下界） ─────────────
pub const BASE_TICKS: [u64; 12] = [100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200, 210];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ledger68Code {
    /// 域越闭集：域名收进四域闭集
    DomainUnknown,
    /// 件越闭集：件名收进三件闭集
    ItemUnknown,
    /// 组合越总表：域件组合收进 12 条签收位
    ComboUnknown,
    /// 时刻倒流：签收时刻须 ≥ 基准时刻，倒流显性拒
    TickReversed,
    /// 重复签收：一域一件一签收，重签先撤旧
    DupSign,
    /// 结论越闭集：结论收进签收/留痕闭集
    VerdictUnknown,
    /// 缺位对平拒：12 签收位缺一即对平拒
    SlotMissing,
    /// 记录窗长度违约：对平窗必须恰 12 条
    WindowBad,
    /// 总账状态倒退：登记→对平→落档单调推进
    PhaseRegress,
    /// 总账状态跳级：未对平不得落档
    PhaseSkip,
    /// 总账行数违约：落档必须恰 14 行
    LedgerRowsBad,
    /// 双跑不一致：排查登记序列随机性后复跑
    NotReproducible,
}

impl Ledger68Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Ledger68Code::DomainUnknown => "域越闭集：域名收进四域闭集再登记",
            Ledger68Code::ItemUnknown => "件越闭集：件名收进三件闭集再登记",
            Ledger68Code::ComboUnknown => "组合越总表：域件组合收进 12 条签收位",
            Ledger68Code::TickReversed => "时刻倒流：签收时刻不早于基准时刻，倒流显性拒",
            Ledger68Code::DupSign => "重复签收：一域一件一签收，重签先撤旧再补",
            Ledger68Code::VerdictUnknown => "结论越闭集：结论收进签收/留痕闭集",
            Ledger68Code::SlotMissing => "缺位对平拒：12 签收位缺一即对平拒，补齐再平",
            Ledger68Code::WindowBad => "记录窗长度违约：对平窗必须恰 12 条",
            Ledger68Code::PhaseRegress => "总账状态倒退：登记→对平→落档单调推进",
            Ledger68Code::PhaseSkip => "总账状态跳级：未对平不得落档",
            Ledger68Code::LedgerRowsBad => "总账行数违约：落档必须恰 14 行",
            Ledger68Code::NotReproducible => "双跑不一致：排查登记序列随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Ledger68Code; 12] = [
        Ledger68Code::DomainUnknown,
        Ledger68Code::ItemUnknown,
        Ledger68Code::ComboUnknown,
        Ledger68Code::TickReversed,
        Ledger68Code::DupSign,
        Ledger68Code::VerdictUnknown,
        Ledger68Code::SlotMissing,
        Ledger68Code::WindowBad,
        Ledger68Code::PhaseRegress,
        Ledger68Code::PhaseSkip,
        Ledger68Code::LedgerRowsBad,
        Ledger68Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Ledger68Code::ALL.len() {
        let c = match Ledger68Code::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── LCG（种子固定 = SEG_BASE，零随机源） ────────────────────────
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    /// 逐步推进并取模返回。
    pub fn next_mod(&mut self, m: usize) -> usize {
        self.state = self
            .state
            .wrapping_mul(1_103_515_245)
            .wrapping_add(12_345);
        let v = (self.state >> 16) as usize;
        if m == 0 {
            return 0;
        }
        v % m
    }
}

/// 总表签收位下标解析：域件组合 → 总表位下标（越闭集返回 None）。
pub fn slot_index(domain: &str, item: &str) -> Option<usize> {
    let mut d = None;
    let mut i = 0usize;
    while i < DOMAINS.len() {
        if let Some(name) = DOMAINS.get(i) {
            if *name == domain {
                d = Some(i);
            }
        }
        i += 1;
    }
    let di = match d {
        Some(v) => v,
        None => return None,
    };
    let mut it = None;
    let mut j = 0usize;
    while j < ITEMS.len() {
        if let Some(name) = ITEMS.get(j) {
            if *name == item {
                it = Some(j);
            }
        }
        j += 1;
    }
    let ii = match it {
        Some(v) => v,
        None => return None,
    };
    Some(di * ITEMS_PER_DOMAIN + ii)
}

/// 基准时刻：总表位下标 → 冻结基准（越界回退 0）。
pub fn base_tick(idx: usize) -> u64 {
    match BASE_TICKS.get(idx) {
        Some(v) => *v,
        None => 0,
    }
}

// ── 总账状态机（登记 → 对平 → 落档，单调推进） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerPhase {
    /// 登记期：逐域逐件四列记录入账
    Enrolling,
    /// 对平期：记录与总表对平 100%
    Balanced,
    /// 落档期：账哈希落档封账
    Sealed,
}

impl LedgerPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            LedgerPhase::Enrolling => 0,
            LedgerPhase::Balanced => 1,
            LedgerPhase::Sealed => 2,
        }
    }
}

/// 单条签收记录（域/件/时刻/结论四列）。
#[derive(Clone, Debug)]
pub struct SignRow {
    pub domain: String,
    pub item: String,
    pub tick: u64,
    pub verdict: String,
}

/// 联签记录总账。
pub struct SignLedger {
    phase: LedgerPhase,
    rows: Vec<SignRow>,
    ledger_digest: u32,
    ledger_text: Option<String>,
}

/// 结论闭集。
pub const VERDICT_SIGN: &str = "签收";
pub const VERDICT_TRACE: &str = "留痕";

impl SignLedger {
    pub fn new() -> SignLedger {
        SignLedger {
            phase: LedgerPhase::Enrolling,
            rows: Vec::new(),
            ledger_digest: 0,
            ledger_text: None,
        }
    }

    pub fn phase(&self) -> LedgerPhase {
        self.phase
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn ledger_digest(&self) -> u32 {
        self.ledger_digest
    }

    pub fn text(&self) -> Option<&str> {
        match self.ledger_text.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }

    /// 签收登记：四列齐备 + 组合命中总表 + 时刻不倒流 + 一位一签。
    pub fn sign(&mut self, domain: &str, item: &str, tick: u64, verdict: &str) -> Result<(), u32> {
        if self.phase.rank() >= LedgerPhase::Balanced.rank() {
            return Err(Ledger68Code::PhaseRegress.val());
        }
        let idx = match slot_index(domain, item) {
            Some(i) => i,
            None => {
                // 区分：域越闭集 / 件越闭集 / 组合越总表。
                let mut dom_known = false;
                let mut i = 0usize;
                while i < DOMAINS.len() {
                    if let Some(n) = DOMAINS.get(i) {
                        if *n == domain {
                            dom_known = true;
                        }
                    }
                    i += 1;
                }
                if !dom_known {
                    return Err(Ledger68Code::DomainUnknown.val());
                }
                let mut item_known = false;
                let mut j = 0usize;
                while j < ITEMS.len() {
                    if let Some(n) = ITEMS.get(j) {
                        if *n == item {
                            item_known = true;
                        }
                    }
                    j += 1;
                }
                if !item_known {
                    return Err(Ledger68Code::ItemUnknown.val());
                }
                return Err(Ledger68Code::ComboUnknown.val());
            }
        };
        if verdict != VERDICT_SIGN && verdict != VERDICT_TRACE {
            return Err(Ledger68Code::VerdictUnknown.val());
        }
        if tick < base_tick(idx) {
            return Err(Ledger68Code::TickReversed.val());
        }
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = match self.rows.get(k) {
                Some(r) => r,
                None => return Err(Ledger68Code::ComboUnknown.val()),
            };
            if r.domain == domain && r.item == item {
                return Err(Ledger68Code::DupSign.val());
            }
            k += 1;
        }
        self.rows.push(SignRow {
            domain: domain.to_string(),
            item: item.to_string(),
            tick,
            verdict: verdict.to_string(),
        });
        Ok(())
    }

    /// 对平：记录与总表 12 位逐位对平（缺位即拒）。
    pub fn balance(&mut self) -> Result<usize, u32> {
        if self.phase != LedgerPhase::Enrolling {
            return Err(Ledger68Code::PhaseRegress.val());
        }
        if self.rows.len() != LEDGER_SLOTS {
            return Err(Ledger68Code::SlotMissing.val());
        }
        let mut seen = [false; LEDGER_SLOTS];
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            let idx = match slot_index(&r.domain, &r.item) {
                Some(v) => v,
                None => return Err(Ledger68Code::ComboUnknown.val()),
            };
            if seen[idx] {
                return Err(Ledger68Code::DupSign.val());
            }
            seen[idx] = true;
            i += 1;
        }
        let mut s = 0usize;
        while s < LEDGER_SLOTS {
            let ok = match seen.get(s) {
                Some(v) => *v,
                None => return Err(Ledger68Code::WindowBad.val()),
            };
            if !ok {
                return Err(Ledger68Code::SlotMissing.val());
            }
            s += 1;
        }
        self.phase = LedgerPhase::Balanced;
        Ok(LEDGER_SLOTS)
    }

    /// 账哈希落档：14 行总账 + FNV-1a 账指纹 + 封账。
    pub fn seal_ledger(&mut self) -> Result<u32, u32> {
        if self.phase != LedgerPhase::Balanced {
            return Err(Ledger68Code::PhaseSkip.val());
        }
        let mut text = String::from("== UNX-F1568 联签记录总账 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            let line = format!("{} {} 时刻{} {} ●", r.domain, r.item, r.tick, r.verdict);
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        text.push_str("== 记录与总表对平 100% · 账哈希落档 ==");
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != LEDGER_ROWS {
            return Err(Ledger68Code::LedgerRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.ledger_text = Some(text);
        self.ledger_digest = digest;
        self.phase = LedgerPhase::Sealed;
        Ok(digest)
    }

    /// LCG 抽验：抽 6 条复验四列齐备且时刻不倒流。
    pub fn sample_audit(&self) -> Result<usize, u32> {
        if self.rows.len() != LEDGER_SLOTS {
            return Err(Ledger68Code::SlotMissing.val());
        }
        let mut lcg = Lcg::new(SEG_BASE);
        let mut ok = 0usize;
        let mut k = 0usize;
        while k < 6 {
            let idx = lcg.next_mod(LEDGER_SLOTS);
            let r = match self.rows.get(idx) {
                Some(r) => r,
                None => return Err(Ledger68Code::ComboUnknown.val()),
            };
            let sidx = match slot_index(&r.domain, &r.item) {
                Some(v) => v,
                None => return Err(Ledger68Code::ComboUnknown.val()),
            };
            if r.tick < base_tick(sidx) || r.verdict.is_empty() {
                return Err(Ledger68Code::TickReversed.val());
            }
            ok += 1;
            k += 1;
        }
        Ok(ok)
    }
}

// ── 好账构建：逐位序贯登记 12 条签收 + 对平 + 落档 ──────────────
pub fn build_ledger_ok() -> SignLedger {
    let mut led = SignLedger::new();
    let mut d = 0usize;
    while d < DOMAIN_COUNT {
        let domain = match DOMAINS.get(d) {
            Some(n) => *n,
            None => break,
        };
        let mut it = 0usize;
        while it < ITEMS_PER_DOMAIN {
            let item = match ITEMS.get(it) {
                Some(n) => *n,
                None => break,
            };
            let idx = match slot_index(domain, item) {
                Some(v) => v,
                None => break,
            };
            let tick = base_tick(idx) + 5;
            let verdict = if it % 2 == 0 { VERDICT_SIGN } else { VERDICT_TRACE };
            let _ = led.sign(domain, item, tick, verdict);
            it += 1;
        }
        d += 1;
    }
    let _ = led.balance();
    let _ = led.seal_ledger();
    led
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78C00 && SEG_BASE <= 0x78DC0
}

/// 四列齐备：12 条记录域/件/时刻/结论逐列非空且入闭集。
fn four_columns_ok() -> bool {
    let led = build_ledger_ok();
    if led.row_count() != LEDGER_SLOTS {
        return false;
    }
    if led.phase() != LedgerPhase::Sealed {
        return false;
    }
    let mut i = 0usize;
    while i < led.row_count() {
        let sidx = match led.rows.get(i) {
            Some(r) => match slot_index(&r.domain, &r.item) {
                Some(v) => v,
                None => return false,
            },
            None => return false,
        };
        let r = match led.rows.get(i) {
            Some(r) => r,
            None => return false,
        };
        if r.verdict != VERDICT_SIGN && r.verdict != VERDICT_TRACE {
            return false;
        }
        if r.tick < base_tick(sidx) {
            return false;
        }
        i += 1;
    }
    true
}

/// 对平 100%：12 位逐位对平、缺位为零。
fn balance_100_ok() -> bool {
    let led = build_ledger_ok();
    if led.row_count() != LEDGER_SLOTS {
        return false;
    }
    let mut seen = [false; LEDGER_SLOTS];
    let mut i = 0usize;
    while i < led.row_count() {
        let r = match led.rows.get(i) {
            Some(r) => r,
            None => return false,
        };
        let idx = match slot_index(&r.domain, &r.item) {
            Some(v) => v,
            None => return false,
        };
        if seen[idx] {
            return false;
        }
        seen[idx] = true;
        i += 1;
    }
    let mut s = 0usize;
    while s < LEDGER_SLOTS {
        if !seen[s] {
            return false;
        }
        s += 1;
    }
    true
}

/// 账哈希落档：14 行、四域三件名齐、指纹非零。
fn ledger_archived_ok() -> bool {
    let led = build_ledger_ok();
    let text = match led.text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != LEDGER_ROWS {
        return false;
    }
    if !text.contains("联签记录总账") {
        return false;
    }
    let mut d = 0usize;
    while d < DOMAINS.len() {
        let name = match DOMAINS.get(d) {
            Some(n) => *n,
            None => break,
        };
        if !text.contains(name) {
            return false;
        }
        d += 1;
    }
    if led.ledger_digest() == 0 {
        return false;
    }
    true
}

/// 时刻倒流注入：低于基准时刻必抓（TickReversed）。
fn inject_reversed_ok() -> bool {
    let mut led = SignLedger::new();
    match led.sign("中断域", "联签件", 1, VERDICT_SIGN) {
        Err(code) => {
            if code != Ledger68Code::TickReversed.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if led.row_count() != 0 {
        return false;
    }
    true
}

/// 陌生域注入：黑户域 / 黑户件 / 黑户结论三路必抓。
fn inject_unknown_ok() -> bool {
    let mut led = SignLedger::new();
    // 黑户域。
    match led.sign("黑户域", "联签件", 200, VERDICT_SIGN) {
        Err(code) => {
            if code != Ledger68Code::DomainUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 黑户件。
    match led.sign("中断域", "黑户件", 200, VERDICT_SIGN) {
        Err(code) => {
            if code != Ledger68Code::ItemUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 黑户结论。
    match led.sign("中断域", "联签件", 200, "口头同意") {
        Err(code) => {
            if code != Ledger68Code::VerdictUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if led.row_count() != 0 {
        return false;
    }
    true
}

/// 重复签收注入：同域同件二签必抓（DupSign）。
fn inject_dup_ok() -> bool {
    let mut led = SignLedger::new();
    if led.sign("计时域", "终验件", 150, VERDICT_SIGN).is_err() {
        return false;
    }
    match led.sign("计时域", "终验件", 160, VERDICT_TRACE) {
        Err(code) => {
            if code != Ledger68Code::DupSign.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if led.row_count() != 1 {
        return false;
    }
    true
}

/// 缺位注入：11 条记录对平必拒（SlotMissing）。
fn inject_missing_ok() -> bool {
    let mut led = SignLedger::new();
    let mut d = 0usize;
    while d < DOMAIN_COUNT {
        let domain = match DOMAINS.get(d) {
            Some(n) => *n,
            None => break,
        };
        let mut it = 0usize;
        while it < ITEMS_PER_DOMAIN {
            let item = match ITEMS.get(it) {
                Some(n) => *n,
                None => break,
            };
            let idx = match slot_index(domain, item) {
                Some(v) => v,
                None => break,
            };
            // 跳过最后一签收位，构造缺位。
            if idx == LEDGER_SLOTS - 1 {
                it += 1;
                continue;
            }
            let tick = base_tick(idx) + 5;
            let _ = led.sign(domain, item, tick, VERDICT_SIGN);
            it += 1;
        }
        d += 1;
    }
    if led.row_count() != LEDGER_SLOTS - 1 {
        return false;
    }
    match led.balance() {
        Err(code) => {
            if code != Ledger68Code::SlotMissing.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 状态机单调：未对平落档拒、封账后再登记拒。
fn phase_monotonic_ok() -> bool {
    let mut led = SignLedger::new();
    // 未对平先落档：跳级拒。
    match led.seal_ledger() {
        Err(code) => {
            if code != Ledger68Code::PhaseSkip.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 好账封账后再签收：倒退拒。
    let mut sealed = build_ledger_ok();
    match sealed.sign("中断域", "联签件", 999, VERDICT_SIGN) {
        Err(code) => {
            if code != Ledger68Code::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 封账后再对平：倒退拒。
    match sealed.balance() {
        Err(code) => {
            if code != Ledger68Code::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// LCG 抽验：好账抽 6 条全过，同种子序列逐位确定。
fn sample_audit_ok() -> bool {
    let led = build_ledger_ok();
    let n = match led.sample_audit() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != 6 {
        return false;
    }
    let mut a = Lcg::new(SEG_BASE);
    let mut b = Lcg::new(SEG_BASE);
    let mut i = 0usize;
    while i < 6 {
        if a.next_mod(LEDGER_SLOTS) != b.next_mod(LEDGER_SLOTS) {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致：好账指纹与总账两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_ledger_ok().ledger_digest();
    let d2 = build_ledger_ok().ledger_digest();
    if d1 != d2 || d1 == 0 {
        return false;
    }
    let t1 = match build_ledger_ok().text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    let t2 = match build_ledger_ok().text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    if t1 != t2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Ledger68Code::ALL.iter() {
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
    let first = match Ledger68Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1568_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1568 联签记录总账");
    s.add_dyn(
        "段位守卫",
        seg_guard_ok(),
        &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落本批段窗 0x78C00..=0x78DC0", SEG_BASE),
    );
    s.add_dyn("四列齐备", four_columns_ok(), "12 条记录域/件/时刻/结论逐列入闭集");
    s.add_dyn("对平100%", balance_100_ok(), "12 签收位逐位对平缺位为零");
    s.add_dyn("账哈希落档", ledger_archived_ok(), "14 行总账落档指纹落账");
    s.add_dyn("时刻倒流注入", inject_reversed_ok(), "低于基准必抓 TickReversed");
    s.add_dyn("陌生域注入", inject_unknown_ok(), "黑户域/件/结论三路必抓");
    s.add_dyn("重复签收注入", inject_dup_ok(), "同域同件二签必抓 DupSign");
    s.add_dyn("缺位注入必抓", inject_missing_ok(), "11 条对平必拒 SlotMissing");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "登记→对平→落档可达，倒退拒");
    s.add_dyn("LCG抽验6条", sample_audit_ok(), "好账抽 6 条全过序列确定");
    s.add_dyn("双跑一致", double_run_ok(), "指纹与总账两次构建逐位相等");
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
        assert_eq!(SEG_BASE, 0x78D00);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1568_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn slot_index_bijection() {
        assert_eq!(LEDGER_SLOTS, 12);
        let mut seen = [false; 12];
        let mut d = 0usize;
        while d < DOMAINS.len() {
            let domain = match DOMAINS.get(d) {
                Some(n) => *n,
                None => break,
            };
            let mut it = 0usize;
            while it < ITEMS.len() {
                let item = match ITEMS.get(it) {
                    Some(n) => *n,
                    None => break,
                };
                let idx = match slot_index(domain, item) {
                    Some(v) => v,
                    None => {
                        assert!(false, "组合必须命中总表");
                        return;
                    }
                };
                assert!(!seen[idx], "组合下标必须两两互异");
                seen[idx] = true;
                it += 1;
            }
            d += 1;
        }
        let mut s = 0usize;
        while s < 12 {
            assert!(seen[s], "位 {} 未被任何组合命中", s);
            s += 1;
        }
    }

    #[test]
    fn base_ticks_nondecreasing() {
        let mut i = 1usize;
        while i < BASE_TICKS.len() {
            let prev = match BASE_TICKS.get(i - 1) {
                Some(v) => *v,
                None => break,
            };
            let cur = match BASE_TICKS.get(i) {
                Some(v) => *v,
                None => break,
            };
            assert!(cur >= prev, "基准时刻必须单调不减");
            i += 1;
        }
    }

    #[test]
    fn full_path_seals() {
        let led = build_ledger_ok();
        assert_eq!(led.row_count(), 12);
        assert_eq!(led.phase(), LedgerPhase::Sealed);
        assert_ne!(led.ledger_digest(), 0);
        assert!(led.text().is_some());
    }

    #[test]
    fn reversed_tick_rejected() {
        let mut led = SignLedger::new();
        match led.sign("内存域", "回执件", 0, VERDICT_SIGN) {
            Err(code) => assert_eq!(code, Ledger68Code::TickReversed.val()),
            Ok(_) => assert!(false, "时刻倒流必须被拒"),
        }
    }

    #[test]
    fn unknown_domain_item_verdict_rejected() {
        let mut led = SignLedger::new();
        assert!(led.sign("幽灵域", "联签件", 200, VERDICT_SIGN).is_err());
        assert!(led.sign("中断域", "幽灵件", 200, VERDICT_SIGN).is_err());
        assert!(led.sign("中断域", "联签件", 200, "随便").is_err());
        assert_eq!(led.row_count(), 0);
    }

    #[test]
    fn duplicate_sign_rejected() {
        let mut led = SignLedger::new();
        assert!(led.sign("调试域", "回执件", 215, VERDICT_SIGN).is_ok());
        match led.sign("调试域", "回执件", 220, VERDICT_TRACE) {
            Err(code) => assert_eq!(code, Ledger68Code::DupSign.val()),
            Ok(_) => assert!(false, "重复签收必须被拒"),
        }
        assert_eq!(led.row_count(), 1);
    }

    #[test]
    fn balance_short_rejected() {
        let mut led = SignLedger::new();
        assert!(led.sign("中断域", "联签件", 105, VERDICT_SIGN).is_ok());
        match led.balance() {
            Err(code) => assert_eq!(code, Ledger68Code::SlotMissing.val()),
            Ok(_) => assert!(false, "缺位对平必须被拒"),
        }
    }

    #[test]
    fn seal_before_balance_rejected() {
        let mut led = SignLedger::new();
        match led.seal_ledger() {
            Err(code) => assert_eq!(code, Ledger68Code::PhaseSkip.val()),
            Ok(_) => assert!(false, "未对平落档必须被拒"),
        }
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_ledger_ok().ledger_digest();
        let d2 = build_ledger_ok().ledger_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(SEG_BASE);
        let mut b = Lcg::new(SEG_BASE);
        assert_eq!(a.next_mod(12), b.next_mod(12));
        assert_eq!(a.next_mod(12), b.next_mod(12));
    }

    #[test]
    fn ledger_rows_and_markers() {
        let led = build_ledger_ok();
        let text = match led.text() {
            Some(t) => t.to_string(),
            None => {
                assert!(false, "总账未落档");
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 14);
        assert!(text.contains("对平 100%"));
        assert!(text.contains("账哈希落档"));
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Ledger68Code::ALL.len(), 12);
        let first = match Ledger68Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Ledger68Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }
}
