//! UNX-F0716 · 断言分层归属账（批/域两级）· 诊断段 0x5980（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0716] 断言分层归属账（批/域两级） <SEG 0x5980>
//!   UNX-F0716-J1 分层归属账落簿（批级断言归各批套件/域级补全归本批
//!   新组）两账对冲全平（批和+域补 = 总数）。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 两级闭集：Level::Batch（批级归属）/ Level::Domain（域级补全），
//!      级外无归属语义；批级归各批套件（批号 1..=MAX_BATCHES），
//!      域级补全归本批新组（组号 1..=MAX_DOMAIN_GROUPS）。
//!   2. 对冲恒等式：批级计数和 + 域级补全计数 == 总落簿数，逐级
//!      复核，失衡即 E_OFFSET_IMBAL（对冲破）。
//!   3. 双账分离：批账与域补账独立落簿、独立审计、独立指纹；
//!      交叉误归（批级条目入域账）显性检出。
//!   4. 归属单调：条目归属一次定终身，重归属必拦（E_REASSIGN）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5980~0x599F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5980 独占 0x5980~0x599F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5980;

/// 0x5981 账已封板仍尝试落簿
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5982 批号越域（合法 1..=MAX_BATCHES）
const E_BATCH_RANGE: u16 = SEG_BASE + 0x2;
/// 0x5983 域组号越域（合法 1..=MAX_DOMAIN_GROUPS）
const E_DGROUP_RANGE: u16 = SEG_BASE + 0x3;
/// 0x5984 对冲破：批和+域补 ≠ 总数
const E_OFFSET_IMBAL: u16 = SEG_BASE + 0x4;
/// 0x5985 重归属：条目二次归属
const E_REASSIGN: u16 = SEG_BASE + 0x5;
/// 0x5986 级号越闭集
const E_LEVEL_RANGE: u16 = SEG_BASE + 0x6;
/// 0x5987 断言名非法：空名或超长
const E_NAME_BAD: u16 = SEG_BASE + 0x7;
/// 0x5988 断言重号：同名二次落簿
const E_ASSERT_DUP: u16 = SEG_BASE + 0x8;
/// 0x5989 交叉误归：批级条目入域账（或反向）
const E_CROSS_ASSIGN: u16 = SEG_BASE + 0x9;
/// 0x598A 账溢出：落簿超定容
const E_LEDGER_FULL: u16 = SEG_BASE + 0xA;
/// 0x598B 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x598C 批账空：批级零条目即对冲
const E_BATCH_EMPTY: u16 = SEG_BASE + 0xB;
/// 0x598D 条目悬空：解引越界
const E_REF_DANGLE: u16 = SEG_BASE + 0xD;
/// 0x598E 空批套件：批号未建套件即归属
const E_SUITE_MISSING: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5980~0x599F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "账已封板：归属冻结，扩账请开新账",
        E_BATCH_RANGE => "批号越域：合法 1..=MAX_BATCHES",
        E_DGROUP_RANGE => "域组号越域：合法 1..=MAX_DOMAIN_GROUPS",
        E_OFFSET_IMBAL => "对冲破：批和+域补 ≠ 总数，归属账失衡",
        E_REASSIGN => "重归属：条目归属一次定终身",
        E_LEVEL_RANGE => "级号越闭集：仅批级/域级",
        E_NAME_BAD => "断言名非法：空名或超 64 字节",
        E_ASSERT_DUP => "断言重号：同名二次落簿拒收",
        E_CROSS_ASSIGN => "交叉误归：批级条目入域账，两账分离破",
        E_LEDGER_FULL => "账溢出：落簿超定容上限",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_BATCH_EMPTY => "批账空：批级零条目无对冲资格",
        E_REF_DANGLE => "条目悬空：解引越界",
        E_SUITE_MISSING => "空批套件：批号未建套件先建套件",
        _ => "未知诊断码（0x5980 段外或保留位）",
    }
}

// ── 常量（来源：两级归属口径与定容） ─────────────────────────────────
/// 批数上限。
pub const MAX_BATCHES: usize = 16;
/// 域组数上限。
pub const MAX_DOMAIN_GROUPS: usize = 8;
/// 落簿定容。
pub const MAX_ENTRIES: usize = 256;
/// 名长度上限。
pub const NAME_MAX: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(MAX_BATCHES == 16);
const _: () = assert!(MAX_DOMAIN_GROUPS == 8);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

fn fnv_str(s: &str) -> u64 {
    let mut h = FNV_OFF;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h = fnv_mix(h, b[i]);
        i += 1;
    }
    h
}

// ── 两级闭集 ─────────────────────────────────────────────────────────
/// 归属两级闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// 批级：断言归各批套件。
    Batch,
    /// 域级：补全归本批新组。
    Domain,
}

impl Level {
    /// 级号。
    pub const fn id(self) -> u8 {
        match self {
            Level::Batch => 0,
            Level::Domain => 1,
        }
    }

    /// 按级号重建。
    pub fn from_id(v: u8) -> Result<Level, u16> {
        match v {
            0 => Ok(Level::Batch),
            1 => Ok(Level::Domain),
            _ => Err(E_LEVEL_RANGE),
        }
    }
}

// ── 归属条目 ─────────────────────────────────────────────────────────
/// 归属条目：名哈希 + 级 + 批号 + 域组号（域级有义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocEntry {
    /// 名 FNV 哈希。
    pub name_fp: u64,
    /// 级。
    pub level: u8,
    /// 批号（批级有义；域级记宿主批号）。
    pub batch: u16,
    /// 域组号（域级 1..=8；批级恒 0）。
    pub dgroup: u8,
}

// ── 批套件 ───────────────────────────────────────────────────────────
/// 批套件：批号 + 套件内断言计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchSuite {
    /// 批号。
    pub batch: u16,
    /// 套件断言计数（批级归属和）。
    pub count: u32,
}

// ── 域补组 ───────────────────────────────────────────────────────────
/// 域补组：组号 + 宿主批号 + 组内补全计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainGroup {
    /// 组号 1..=8。
    pub group: u16,
    /// 宿主批号。
    pub host_batch: u16,
    /// 组内补全计数。
    pub count: u32,
}

// ── 分层归属账 ───────────────────────────────────────────────────────
/// 断言分层归属账：批套件账 + 域补组账 + 对冲恒等审计。
pub struct LayeredLedger {
    /// 批套件账（批号索引派发）。
    suites: Vec<BatchSuite>,
    /// 域补组账。
    dgroups: Vec<DomainGroup>,
    /// 归属条目账。
    entries: Vec<AllocEntry>,
    /// 总落簿数。
    total: u32,
    /// 封板位。
    sealed: bool,
}

impl LayeredLedger {
    /// 新账。
    pub fn new() -> Self {
        LayeredLedger {
            suites: Vec::new(),
            dgroups: Vec::new(),
            entries: Vec::new(),
            total: 0,
            sealed: false,
        }
    }

    /// 建批套件。
    pub fn open_suite(&mut self, batch: u16) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if batch < 1 || batch as usize > MAX_BATCHES {
            return Err(E_BATCH_RANGE);
        }
        let mut i = 0usize;
        while i < self.suites.len() {
            if self.suites[i].batch == batch {
                return Err(E_REASSIGN);
            }
            i += 1;
        }
        self.suites.push(BatchSuite {
            batch,
            count: 0,
        });
        Ok(())
    }

    /// 建域补组。
    pub fn open_dgroup(&mut self, group: u16, host_batch: u16) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if group < 1 || group as usize > MAX_DOMAIN_GROUPS {
            return Err(E_DGROUP_RANGE);
        }
        if host_batch < 1 || host_batch as usize > MAX_BATCHES {
            return Err(E_BATCH_RANGE);
        }
        let mut i = 0usize;
        while i < self.dgroups.len() {
            if self.dgroups[i].group == group {
                return Err(E_REASSIGN);
            }
            i += 1;
        }
        self.dgroups.push(DomainGroup {
            group,
            host_batch,
            count: 0,
        });
        Ok(())
    }

    /// 批级归属：条目落批账（套件必须已建）。
    pub fn assign_batch(&mut self, name: &str, batch: u16) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.entries.len() >= MAX_ENTRIES {
            return Err(E_LEDGER_FULL);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if batch < 1 || batch as usize > MAX_BATCHES {
            return Err(E_BATCH_RANGE);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].name_fp == fp {
                return Err(E_REASSIGN);
            }
            i += 1;
        }
        // 套件必须已建。
        let mut s = 0usize;
        while s < self.suites.len() {
            if self.suites[s].batch == batch {
                break;
            }
            s += 1;
        }
        if s == self.suites.len() {
            return Err(E_SUITE_MISSING);
        }
        self.entries.push(AllocEntry {
            name_fp: fp,
            level: Level::Batch.id(),
            batch,
            dgroup: 0,
        });
        self.suites[s].count = self.suites[s].count.wrapping_add(1);
        self.total = self.total.wrapping_add(1);
        Ok(())
    }

    /// 域级归属：补全条目落域账（组必须已建；批级重复名拦）。
    pub fn assign_domain(&mut self, name: &str, group: u16) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.entries.len() >= MAX_ENTRIES {
            return Err(E_LEDGER_FULL);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if group < 1 || group as usize > MAX_DOMAIN_GROUPS {
            return Err(E_DGROUP_RANGE);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].name_fp == fp {
                // 已归属：重归属/交叉误归检出。
                return Err(E_REASSIGN);
            }
            i += 1;
        }
        let mut g = 0usize;
        while g < self.dgroups.len() {
            if self.dgroups[g].group == group {
                break;
            }
            g += 1;
        }
        if g == self.dgroups.len() {
            return Err(E_SUITE_MISSING);
        }
        self.entries.push(AllocEntry {
            name_fp: fp,
            level: Level::Domain.id(),
            batch: self.dgroups[g].host_batch,
            dgroup: group as u8,
        });
        self.dgroups[g].count = self.dgroups[g].count.wrapping_add(1);
        self.total = self.total.wrapping_add(1);
        Ok(())
    }

    /// 总落簿数。
    pub const fn total(&self) -> u32 {
        self.total
    }

    /// 批级和。
    pub fn batch_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.suites.len() {
            s = s.wrapping_add(self.suites[i].count);
            i += 1;
        }
        s
    }

    /// 域补和。
    pub fn domain_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.dgroups.len() {
            s = s.wrapping_add(self.dgroups[i].count);
            i += 1;
        }
        s
    }

    /// 对冲审计：批和 + 域补 == 总数（判据恒等式）；另核双账分离
    /// （批级条目 dgroup 恒 0；域级条目 dgroup 恒 1..=8）。
    pub fn audit_offset(&self) -> Result<(), u16> {
        if self.suites.is_empty() {
            return Err(E_BATCH_EMPTY);
        }
        if self.batch_sum().wrapping_add(self.domain_sum()) != self.total {
            return Err(E_OFFSET_IMBAL);
        }
        // 双账分离复列。
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = self.entries[i];
            if e.level == Level::Batch.id() {
                if e.dgroup != 0 {
                    return Err(E_CROSS_ASSIGN);
                }
            } else if e.dgroup == 0 {
                return Err(E_CROSS_ASSIGN);
            }
            i += 1;
        }
        Ok(())
    }

    /// 归属条目解引。
    pub fn entry_at(&self, idx: usize) -> Result<AllocEntry, u16> {
        if idx >= self.entries.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(self.entries[idx])
    }

    /// 批套件解引。
    pub fn suite_at(&self, batch: u16) -> Result<BatchSuite, u16> {
        if batch < 1 || batch as usize > MAX_BATCHES {
            return Err(E_BATCH_RANGE);
        }
        let mut i = 0usize;
        while i < self.suites.len() {
            if self.suites[i].batch == batch {
                return Ok(self.suites[i]);
            }
            i += 1;
        }
        Err(E_SUITE_MISSING)
    }

    /// 封板：对冲审计过后落锁。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.audit_offset()?;
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.suites.len() {
            h = fnv_mix(h, (self.suites[i].batch & 0xFF) as u8 | 0x10);
            h = fnv_mix(h, (self.suites[i].count & 0xFF) as u8);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.dgroups.len() {
            h = fnv_mix(h, (self.dgroups[j].group & 0xFF) as u8 | 0x20);
            h = fnv_mix(h, (self.dgroups[j].count & 0xFF) as u8);
            j += 1;
        }
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = self.entries[k];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((e.name_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, e.level | 0x30);
            h = fnv_mix(h, (e.batch & 0xFF) as u8 | 0x40);
            h = fnv_mix(h, e.dgroup | 0x50);
            k += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.total == 0 {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("分层归属账: 总=");
        out.push_str(&itoa(self.total as u64));
        out.push_str(" 批和=");
        out.push_str(&itoa(self.batch_sum() as u64));
        out.push_str(" 域补=");
        out.push_str(&itoa(self.domain_sum() as u64));
        out.push_str(" 套件=");
        out.push_str(&itoa(self.suites.len() as u64));
        out.push_str(" 域组=");
        out.push_str(&itoa(self.dgroups.len() as u64));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn lname(i: usize) -> String {
    let mut s = String::new();
    s.push_str("layer_");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：2 批套件 + 2 域补组 → 批 12 + 域补 4 = 16 → 对冲全平 ——
    let mut led = LayeredLedger::new();
    led.open_suite(1)?;
    led.open_suite(2)?;
    led.open_dgroup(1, 1)?;
    led.open_dgroup(2, 2)?;
    // 批级 12 条（批 1×6 + 批 2×6）。
    let mut i = 0usize;
    while i < 6 {
        led.assign_batch(&lname(i), 1)?;
        led.assign_batch(&lname(10 + i), 2)?;
        i += 1;
    }
    // 域补 4 条（组 1×2 + 组 2×2）。
    i = 0;
    while i < 2 {
        led.assign_domain(&lname(20 + i), 1)?;
        led.assign_domain(&lname(30 + i), 2)?;
        i += 1;
    }
    if led.total() != 16 {
        return Err(E_OFFSET_IMBAL);
    }
    if led.batch_sum() != 12 || led.domain_sum() != 4 {
        return Err(E_OFFSET_IMBAL);
    }
    led.audit_offset()?;
    let fp = led.seal()?;
    if fp == 0 {
        return Err(E_OFFSET_IMBAL);
    }
    // 封板后归属必拦。
    if led.assign_batch("late", 1) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    let sm = led.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }
    // 条目解引抽查：首条批级 dgroup==0。
    let e0 = led.entry_at(0)?;
    if e0.level != Level::Batch.id() || e0.dgroup != 0 {
        return Err(E_CROSS_ASSIGN);
    }
    // 批级条目末尾域级条目 dgroup!=0。
    let e15 = led.entry_at(15)?;
    if e15.level != Level::Domain.id() || e15.dgroup == 0 {
        return Err(E_CROSS_ASSIGN);
    }
    if led.entry_at(16) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }
    // 套件解引。
    let s1 = led.suite_at(1)?;
    if s1.count != 6 {
        return Err(E_OFFSET_IMBAL);
    }
    if led.suite_at(3) != Err(E_SUITE_MISSING) {
        return Err(E_SUITE_MISSING);
    }

    // —— 对冲破注入：域补计数拉爆 ——
    let mut bad = LayeredLedger::new();
    bad.open_suite(1)?;
    bad.open_dgroup(1, 1)?;
    let mut j = 0usize;
    while j < 4 {
        bad.assign_batch(&lname(100 + j), 1)?;
        j += 1;
    }
    bad.assign_domain(&lname(200), 1)?;
    bad.dgroups[0].count = bad.dgroups[0].count.wrapping_add(1);
    if bad.audit_offset() != Err(E_OFFSET_IMBAL) {
        return Err(E_OFFSET_IMBAL);
    }

    // —— 交叉误归注入：批级条目 dgroup 置非零 ——
    let mut bad2 = LayeredLedger::new();
    bad2.open_suite(1)?;
    bad2.open_dgroup(1, 1)?;
    bad2.assign_batch(&lname(300), 1)?;
    bad2.entries[0].dgroup = 1;
    if bad2.audit_offset() != Err(E_CROSS_ASSIGN) {
        return Err(E_CROSS_ASSIGN);
    }

    // —— 重归属注入：同名二次归属 ——
    let mut bad3 = LayeredLedger::new();
    bad3.open_suite(1)?;
    bad3.open_dgroup(1, 1)?;
    bad3.assign_batch(&lname(400), 1)?;
    if bad3.assign_batch(&lname(400), 1) != Err(E_REASSIGN) {
        return Err(E_REASSIGN);
    }
    if bad3.assign_domain(&lname(400), 1) != Err(E_REASSIGN) {
        return Err(E_REASSIGN);
    }

    // —— 错误路径样本 ——
    if Level::from_id(2) != Err(E_LEVEL_RANGE) {
        return Err(E_LEVEL_RANGE);
    }
    let mut err = LayeredLedger::new();
    if err.open_suite(0) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    if err.open_suite(MAX_BATCHES as u16 + 1) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    if err.open_dgroup(0, 1) != Err(E_DGROUP_RANGE) {
        return Err(E_DGROUP_RANGE);
    }
    if err.open_dgroup(MAX_DOMAIN_GROUPS as u16 + 1, 1) != Err(E_DGROUP_RANGE) {
        return Err(E_DGROUP_RANGE);
    }
    if err.open_dgroup(1, 0) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    // 套件重建。
    err.open_suite(1)?;
    if err.open_suite(1) != Err(E_REASSIGN) {
        return Err(E_REASSIGN);
    }
    // 名非法。
    if err.assign_batch("", 1) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    if err.assign_domain("", 1) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    // 未建套件即归属。
    if err.assign_batch("x", 2) != Err(E_SUITE_MISSING) {
        return Err(E_SUITE_MISSING);
    }
    // 未建域组即归属。
    if err.assign_domain("y", 2) != Err(E_SUITE_MISSING) {
        return Err(E_SUITE_MISSING);
    }
    // 批账空对冲拦。
    err.open_dgroup(1, 1)?;
    if err.audit_offset() != Err(E_BATCH_EMPTY) {
        return Err(E_BATCH_EMPTY);
    }
    // 摘要空账拦。
    if err.summary() != Err(E_SUMMARY_EMPTY) {
        return Err(E_SUMMARY_EMPTY);
    }

    // 诊断表抽查。
    if diag_detail(E_OFFSET_IMBAL) == "未知诊断码（0x5980 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5980 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
