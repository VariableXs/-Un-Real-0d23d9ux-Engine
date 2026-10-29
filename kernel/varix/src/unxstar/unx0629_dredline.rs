//! unx0629_dredline —— UNX-F0629 域红线总清单（全批禁令汇总）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0629 · 域红线总清单（全批禁令汇总） · 目标 ≥220 行
//! 判据原文摘录：红线清单账齐（12 域红线汇总——B13–B31 redline 注记
//!   逐批收），逐条可达断言（12/12）。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 12 域红线名册闭集写死（域号/域名/禁令条文三列），一条域一
//!      条红线，重号拒收。
//!   2. 逐条可达断言：每条红线必须能在断言机上走通（断言 = 禁令
//!      编译成整数判式，判式结果为"禁令在账"），12/12 全达才账齐。
//!   3. 逐批收账语义：红线注记按 B13..B31 逐批汇入，批号缺失的红线
//!      不可达（来源批闭集核）。
//!   4. 注入红检出：任何一条红线被"摘除"或条文清空，可达断言必红。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5060 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5060 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5060;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x506F;

/// XcCode：域号越界。
pub const XC_BAD_DOMAIN: u16 = SEG_BASE + 1; // 0x5061 域号 0..12 闭集外
/// XcCode：红线重复。
pub const XC_DUP_REDLINE: u16 = SEG_BASE + 2; // 0x5062 一域一条不许重收
/// XcCode：条文为空。
pub const XC_EMPTY_CLAUSE: u16 = SEG_BASE + 3; // 0x5063 禁令条文不可空
/// XcCode：来源批越界。
pub const XC_BAD_SRC_BATCH: u16 = SEG_BASE + 4; // 0x5064 B13..B31 闭集外
/// XcCode：可达断言失败。
pub const XC_REACH_FAIL: u16 = SEG_BASE + 5; // 0x5065 断言不可达
/// XcCode：清单未账齐。
pub const XC_LIST_SHORT: u16 = SEG_BASE + 6; // 0x5066 12/12 未收齐

/// 12 域红线数。
pub const REDLINE_N: usize = 12;
/// 来源批数（B13..B31）。
pub const SRC_BATCH_N: usize = 19;

/// 12 域名册（域号 → 域名）。
pub const DOMAIN_NAMES: [&str; REDLINE_N] = [
    "时钟域",
    "内存域",
    "中断域",
    "ACPI 域",
    "PCI 域",
    "显示域",
    "输入域",
    "ktest 域",
    "日志域",
    "参数域",
    "initrd 域",
    "封账域",
];

/// 12 域红线禁令条文（闭集写死，逐条对应域名）。
pub const CLAUSES: [&str; REDLINE_N] = [
    "禁：时钟源未落账即消费",
    "禁：内存区未对账即释放",
    "禁：中断路由未对冲即使能",
    "禁：ACPI 表未校验即进 S3",
    "禁：PCI 配置空间未封账即写",
    "禁：显示模式未断言即切换",
    "禁：输入事件未入账即分发",
    "禁：ktest 红项未复演即放行",
    "禁：日志序号断链即合流",
    "禁：参数键越白名单即生效",
    "禁：initrd 在场却走空档路径",
    "禁：封账后未走勘误通道直改",
];

/// 单条红线账目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedlineRow {
    /// 域号（0..12）。
    pub domain: usize,
    /// 域名（须与名册一致）。
    pub domain_name: String,
    /// 禁令条文（非空）。
    pub clause: String,
    /// 来源批号（0..19，对应 B13..B31）。
    pub src_batch: usize,
}

impl RedlineRow {
    /// 账目有效性：条文非空 + 域名一致。
    pub fn valid(&self) -> bool {
        !self.clause.is_empty() && self.domain_name == DOMAIN_NAMES[self.domain]
    }
}

/// 可达断言机：一条红线的"可达" = 闭集内 + 名册一致 + 条文非空 +
/// 来源批在 B13..B31 闭集。全过 = 可达。
pub fn reachable(r: &RedlineRow) -> bool {
    r.valid() && r.src_batch < SRC_BATCH_N
}

/// 域红线总清单账本。
#[derive(Clone, Debug, Default)]
pub struct RedlineBook {
    /// 12 槽（None = 未收）。
    slots: [Option<RedlineRow>; REDLINE_N],
}

impl RedlineBook {
    /// 空清单。
    pub fn new() -> Self {
        RedlineBook {
            slots: [
                None, None, None, None, None, None, None, None, None, None, None, None,
            ],
        }
    }

    /// 已收条数。
    pub fn collected(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 守恒：已收 + 空 = 12。
    pub fn conserve_ok(&self) -> bool {
        self.collected() + self.slots.iter().filter(|s| s.is_none()).count() == REDLINE_N
    }

    /// 逐批收一条红线（号闭集 + 去重 + 条文非空 + 批闭集）。
    pub fn collect(&mut self, domain: usize, clause: &str, src_batch: usize) -> Result<(), u16> {
        if domain >= REDLINE_N {
            return Err(XC_BAD_DOMAIN);
        }
        if self.slots[domain].is_some() {
            return Err(XC_DUP_REDLINE);
        }
        if clause.is_empty() {
            return Err(XC_EMPTY_CLAUSE);
        }
        if src_batch >= SRC_BATCH_N {
            return Err(XC_BAD_SRC_BATCH);
        }
        self.slots[domain] = Some(RedlineRow {
            domain,
            domain_name: DOMAIN_NAMES[domain].to_string(),
            clause: clause.to_string(),
            src_batch,
        });
        Ok(())
    }

    /// 按域号取红线。
    pub fn at(&self, domain: usize) -> Option<&RedlineRow> {
        if domain >= REDLINE_N {
            return None;
        }
        self.slots[domain].as_ref()
    }

    /// 逐条可达断言：12/12 全达才平（红定位最小失败域号——经 Result 语义）。
    pub fn all_reachable(&self) -> Result<usize, u16> {
        if self.collected() != REDLINE_N {
            return Err(XC_LIST_SHORT);
        }
        for i in 0..REDLINE_N {
            match &self.slots[i] {
                Some(r) => {
                    if !reachable(r) {
                        return Err(XC_REACH_FAIL);
                    }
                }
                None => return Err(XC_LIST_SHORT),
            }
        }
        Ok(REDLINE_N)
    }

    /// 来源批覆盖核：12 条红线的来源批去重数 ≥ 12（每域独立收账）。
    pub fn src_coverage(&self) -> Result<usize, u16> {
        let mut seen = [false; SRC_BATCH_N];
        for r in self.slots.iter().flatten() {
            seen[r.src_batch] = true;
        }
        let n = seen.iter().filter(|s| **s).count();
        if n < REDLINE_N {
            return Err(XC_REACH_FAIL);
        }
        Ok(n)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "域红线总清单 · 已收 {}/{} · 守恒={}",
            self.collected(),
            REDLINE_N,
            self.conserve_ok()
        );
        for i in 0..REDLINE_N {
            match &self.slots[i] {
                Some(r) => s.push_str(&alloc::format!(
                    "\n  域{} {} ← {} ：{}",
                    r.domain, r.domain_name, BATCH_TAG[r.src_batch], r.clause
                )),
                None => s.push_str(&alloc::format!("\n  域{} 未收", i)),
            }
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// B13..B31 批标签（来源批显示用）。
pub const BATCH_TAG: [&str; SRC_BATCH_N] = [
    "B13", "B14", "B15", "B16", "B17", "B18", "B19", "B20", "B21", "B22",
    "B23", "B24", "B25", "B26", "B27", "B28", "B29", "B30", "B31",
];

/// 标准清单构造器：域 i 的来源批 = 19 - i（保证 12 域来源批互异）。
pub fn build_standard_list() -> Result<RedlineBook, u16> {
    let mut b = RedlineBook::new();
    for i in 0..REDLINE_N {
        let sb = if 19 - i < SRC_BATCH_N { 19 - i } else { 0 };
        b.collect(i, CLAUSES[i], sb)?;
    }
    b.all_reachable()?;
    Ok(b)
}

/// 自例程：主路径（12/12 收齐→可达→批覆盖）+ 错误路径（越界/重收/空条文/摘除/批越界）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let list = build_standard_list()?;
    if list.all_reachable()? != REDLINE_N {
        return Err(XC_REACH_FAIL);
    }
    if list.src_coverage()? < REDLINE_N {
        return Err(XC_REACH_FAIL);
    }
    if !list.conserve_ok() {
        return Err(XC_LIST_SHORT);
    }
    if list.fingerprint() == 0 {
        return Err(XC_REACH_FAIL);
    }

    // 错误路径 1：域号越界。
    let mut b1 = RedlineBook::new();
    if b1.collect(REDLINE_N, "x", 0) != Err(XC_BAD_DOMAIN) {
        return Err(XC_BAD_DOMAIN);
    }
    // 错误路径 2：重收。
    let mut b2 = build_standard_list()?;
    if b2.collect(0, "再收", 0) != Err(XC_DUP_REDLINE) {
        return Err(XC_DUP_REDLINE);
    }
    // 错误路径 3：空条文。
    let mut b3 = RedlineBook::new();
    if b3.collect(0, "", 0) != Err(XC_EMPTY_CLAUSE) {
        return Err(XC_EMPTY_CLAUSE);
    }
    // 错误路径 4：来源批越界。
    let mut b4 = RedlineBook::new();
    if b4.collect(0, CLAUSES[0], SRC_BATCH_N) != Err(XC_BAD_SRC_BATCH) {
        return Err(XC_BAD_SRC_BATCH);
    }
    // 错误路径 5：摘除一条 → 清单未齐。
    let mut b5 = build_standard_list()?;
    b5.slots[3] = None;
    if b5.all_reachable() != Err(XC_LIST_SHORT) {
        return Err(XC_LIST_SHORT);
    }
    // 错误路径 6：条文清空 → 可达失败。
    let mut b6 = build_standard_list()?;
    if let Some(r) = &mut b6.slots[5] {
        r.clause = String::new();
    }
    if b6.all_reachable() != Err(XC_REACH_FAIL) {
        return Err(XC_REACH_FAIL);
    }
    // 错误路径 7：来源批收窄（多域同批 → 覆盖核红）。
    let mut b7 = RedlineBook::new();
    for i in 0..REDLINE_N {
        b7.collect(i, CLAUSES[i], 0)?;
    }
    if b7.all_reachable().is_err() {
        // 可达面先过（批 0 在闭集内）。
        return Err(XC_REACH_FAIL);
    }
    if b7.src_coverage() != Err(XC_REACH_FAIL) {
        return Err(XC_REACH_FAIL);
    }
    Ok(0)
}
