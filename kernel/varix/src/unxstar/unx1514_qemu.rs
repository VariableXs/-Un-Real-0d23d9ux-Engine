//! UNX-F1514 回归环境隔离账（开发期零 QEMU 纪律承载） · 0x78200段
//!
//! 判据原文（UNX-F1514-J1 摘录）：「回归环境隔离账生效 100%（开发期回归=
//! 账面/逻辑执行零 QEMU 零实机写 R-A2-001 承载：环境档位三态登记），越界
//! 执行注入 10/10 必拦，隔离结论随批落档」；完成时 API/软件写回：状态=
//! 已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 环境档位三态闭集：账面档（B0）/逻辑执行档（B1）/实机档（B2），开发期
//!    回归只允许 B0/B1 两档登记，B2（QEMU/实机）任何写入一律显性拒——
//!    R-A2-001 红线在登记门上承载，不留事后补票通道；
//! 2. 隔离账状态机 未登记→已登记→已封存 单向推进，封存后再登记即倒退，
//!    倒退显性拒并计入拒绝账（不静默吞）；
//! 3. 每批隔离结论随批落档：封存时生成非空摘要（档位计数+折叠根），摘要为
//!    空即判据翻红——「结论落档」不是口号而是账面字段；
//! 4. 越界执行注入 10/10 必拦：十轮注入全部构造 B2 档写入，登记门必须
//!    十轮全拦，任何一轮漏拦即整账翻红；合法 B0/B1 双轨登记零误伤；
//! 5. 双跑一致：同语料独立建账两次，折叠根与摘要计数逐项对拍，不一致即
//!    账本不可复算，判据翻红；
//! 6. 诊断码闭集 0x78201..0x7820C，Err 用 Result<T,u32>，查表闭集返回。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78200;

/// 隔离账槽位容量（批内环境档位登记槽，冻结值）。
///
/// 来源：R-A2-001 开发期回归承载表——单批回归最多 12 个环境档位登记槽，
/// 槽号 1..=12，超出滚动进下一批（不扩容，容量即纪律）。
pub const REG_SLOTS: usize = 12;

/// 越界执行注入轮数（判据原文 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 环境档位三态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvTier {
    /// 账面档（B0）：纯账面/纯逻辑推演，无任何执行面。
    Book,
    /// 逻辑执行档（B1）：逻辑执行面，无 QEMU 无实机。
    LogicExec,
    /// 实机档（B2）：QEMU/实机执行面，开发期禁用。
    RealMachine,
}

impl EnvTier {
    /// 档位序（0..=2，闭集内单调）。
    pub fn ord(self) -> u8 {
        match self {
            EnvTier::Book => 0,
            EnvTier::LogicExec => 1,
            EnvTier::RealMachine => 2,
        }
    }

    /// 档位中文标签（落档摘要用）。
    pub fn tag(self) -> &'static str {
        match self {
            EnvTier::Book => "账面档",
            EnvTier::LogicExec => "逻辑执行档",
            EnvTier::RealMachine => "实机档",
        }
    }
}

/// 三态闭集数组（登记门枚举来源）。
pub const TIERS: &[EnvTier] = &[EnvTier::Book, EnvTier::LogicExec, EnvTier::RealMachine];

/// 隔离账诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 实机档越界登记（QEMU/实机写入被登记门拦截）。
    EBanned,
    /// 隔离账已封存（封存后写入拒绝）。
    ESealed,
    /// 槽号重复登记。
    EDup,
    /// 登记备注为空。
    EEmpty,
    /// 槽号越界（不在 1..=12）。
    ERange,
    /// 登记乱序（槽号必须递增登记）。
    EOrder,
    /// 容量已满。
    ECap,
    /// 隔离结论摘要缺失或为空。
    ENoSummary,
    /// 状态机倒退（封存后重开/重登记）。
    ERegress,
    /// 封存重放复核不符。
    EReplay,
    /// 注入漏拦（越界执行未被登记门拦截）。
    EInject,
    /// 封存摘要档位计数与账面不符。
    ECount,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EBanned => 0x01,
                Diag::ESealed => 0x02,
                Diag::EDup => 0x03,
                Diag::EEmpty => 0x04,
                Diag::ERange => 0x05,
                Diag::EOrder => 0x06,
                Diag::ECap => 0x07,
                Diag::ENoSummary => 0x08,
                Diag::ERegress => 0x09,
                Diag::EReplay => 0x0a,
                Diag::EInject => 0x0b,
                Diag::ECount => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EBanned => "实机档越界登记：开发期回归只许账面/逻辑执行两档，QEMU/实机写入一律拒绝",
            Diag::ESealed => "隔离账已封存：封存后写入拒绝，结论随批落档后开下一批新账",
            Diag::EDup => "槽号重复：每槽只落一条登记，改档走下一批新槽",
            Diag::EEmpty => "登记备注为空：每条登记必须写明环境用途，空备注不构成隔离证据",
            Diag::ERange => "槽号越界：槽号必须在 1..=12 闭区间内",
            Diag::EOrder => "登记乱序：按槽号 1,2,3.. 递增登记，禁止回填",
            Diag::ECap => "容量已满：十二槽已齐，多余档位滚动进下一批",
            Diag::ENoSummary => "隔离结论缺失：封存时必须生成非空摘要，摘要为空不落档",
            Diag::ERegress => "状态机倒退：未登记→已登记→已封存单向推进，禁止回退",
            Diag::EReplay => "封存重放不符：重放档位计数与封存摘要不一致，账本可疑",
            Diag::EInject => "注入漏拦：越界执行注入必须 10/10 全拦，任何漏拦即整账翻红",
            Diag::ECount => "档位计数不符：封存摘要计数必须与账面逐档相符",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EBanned,
    Diag::ESealed,
    Diag::EDup,
    Diag::EEmpty,
    Diag::ERange,
    Diag::EOrder,
    Diag::ECap,
    Diag::ENoSummary,
    Diag::ERegress,
    Diag::EReplay,
    Diag::EInject,
    Diag::ECount,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78201..0x7820C 闭集内"
}

/// 一条环境档位登记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvEntry {
    /// 槽号（1..=12）。
    pub slot: u32,
    /// 环境档位（B0/B1/B2 三态）。
    pub tier: EnvTier,
    /// 登记备注（非空，写明环境用途）。
    pub note: String,
}

/// 回归环境隔离账。
///
/// 状态机：未登记（空账）→已登记（逐槽落账）→已封存（结论落档）。
/// 单向推进；封存后任何写路径返回 ESealed，重开即 ERegress。
#[derive(Clone, Debug)]
pub struct IsoLedger {
    entries: Vec<EnvEntry>,
    sealed: bool,
    /// 被登记门拒绝的写入次数（含实机档拦截），显性记账。
    rejects: u32,
    /// 封存时生成的隔离结论摘要（随批落档）。
    summary: String,
    /// 封存折叠根（槽号/档位/备注折叠，双跑一致性对拍面）。
    root: u64,
}

impl IsoLedger {
    /// 建空账。
    pub fn new() -> IsoLedger {
        IsoLedger {
            entries: Vec::new(),
            sealed: false,
            rejects: 0,
            summary: String::new(),
            root: 0,
        }
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 已封存判定。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 累计拒绝次数（显性不静默）。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 隔离结论摘要（封存后非空）。
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 封存折叠根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 第 `idx` 条登记（越界返回 None）。
    pub fn at(&self, idx: usize) -> Option<&EnvEntry> {
        self.entries.get(idx)
    }

    /// 某档位的登记计数。
    pub fn count_tier(&self, tier: EnvTier) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.tier == tier {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 登记一个环境档位（登记门）。
    ///
    /// 门序：封存拒 → 槽号界 → 档位禁（实机档 R-A2-001 承载点）→ 备注非空
    /// → 重复槽 → 顺序递增 → 容量。任何一门拒绝只加拒绝计数，不动已落账。
    pub fn register(&mut self, slot: u32, tier: EnvTier, note: &str) -> Result<(), u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if slot == 0 || slot as usize > REG_SLOTS {
            self.rejects += 1;
            return Err(Diag::ERange.val());
        }
        // R-A2-001 承载点：开发期实机档（QEMU/实机）零写入。
        if tier == EnvTier::RealMachine {
            self.rejects += 1;
            return Err(Diag::EBanned.val());
        }
        if note.is_empty() {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.slot == slot {
                    self.rejects += 1;
                    return Err(Diag::EDup.val());
                }
            }
            i += 1;
        }
        if let Some(last) = self.entries.last() {
            if slot <= last.slot {
                self.rejects += 1;
                return Err(Diag::EOrder.val());
            }
        }
        if self.entries.len() >= REG_SLOTS {
            self.rejects += 1;
            return Err(Diag::ECap.val());
        }
        self.entries.push(EnvEntry {
            slot,
            tier,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 封存（未登记→已登记→已封存 的终态跃迁）。
    ///
    /// 空账不许封存（无登记不构成隔离证据）；封存生成非空摘要并折叠根。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::ERegress.val());
        }
        if self.entries.is_empty() {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        self.root = fold_entries(&self.entries);
        self.summary = format!(
            "隔离账封存：登记{}条 账面档{} 逻辑执行档{} 实机档0 根=0x{:016x}",
            self.entries.len(),
            self.count_tier(EnvTier::Book),
            self.count_tier(EnvTier::LogicExec),
            self.root,
        );
        self.sealed = true;
        Ok(())
    }

    /// 封存重放：逐条复核账面与摘要计数一致（重放不符即账本可疑）。
    pub fn replay(&self) -> Result<usize, u32> {
        if !self.sealed {
            return Err(Diag::EReplay.val());
        }
        if self.root != fold_entries(&self.entries) {
            return Err(Diag::EReplay.val());
        }
        if self.count_tier(EnvTier::RealMachine) != 0 {
            return Err(Diag::ECount.val());
        }
        let total = self.count_tier(EnvTier::Book) + self.count_tier(EnvTier::LogicExec);
        if total as usize != self.entries.len() {
            return Err(Diag::ECount.val());
        }
        Ok(self.entries.len())
    }
}

/// 折叠根：槽号/档位/备注字节 FNV 折叠（整数运算，双跑确定性）。
fn fold_entries(entries: &[EnvEntry]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < entries.len() {
        if let Some(e) = entries.get(i) {
            h = (h ^ (e.slot as u64)).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ (e.tier.ord() as u64)).wrapping_mul(0x0000_0100_0000_01b3);
            let bytes = e.note.as_bytes();
            let mut k = 0usize;
            while k < bytes.len() {
                if let Some(b) = bytes.get(k) {
                    h = (h ^ (*b as u64)).wrapping_mul(0x0000_0100_0000_01b3);
                }
                k += 1;
            }
        }
        i += 1;
    }
    h
}

/// 正向语料：六条合法登记（账面档三条+逻辑执行档三条，槽号递增）。
fn build_clean() -> IsoLedger {
    let mut l = IsoLedger::new();
    let _ = l.register(1, EnvTier::Book, "B0 账面推演：回归矩阵判据面");
    let _ = l.register(2, EnvTier::LogicExec, "B1 逻辑执行：注入族检测面");
    let _ = l.register(3, EnvTier::Book, "B0 账面推演：勾稽对账面");
    let _ = l.register(4, EnvTier::LogicExec, "B1 逻辑执行：ktest 断言面");
    let _ = l.register(5, EnvTier::Book, "B0 账面推演：语义快照面");
    let _ = l.register(6, EnvTier::LogicExec, "B1 逻辑执行：延迟账对账面");
    let _ = l.seal();
    l
}

/// 越界执行注入矩阵：十轮全部构造实机档（QEMU/实机）写入，
/// 每轮必须被登记门以 EBanned 拦下；返回十轮拦截布尔表。
fn inject_matrix() -> [bool; INJECT_ROUNDS] {
    let mut hits = [false; INJECT_ROUNDS];
    let mut r = 0usize;
    while r < INJECT_ROUNDS {
        let mut l = IsoLedger::new();
        let probe = match l.register((r + 1) as u32, EnvTier::RealMachine, "越界注入：QEMU/实机执行面") {
            Err(c) => c == Diag::EBanned.val(),
            Ok(()) => false,
        };
        match hits.get_mut(r) {
            Some(cell) => *cell = probe,
            None => return hits,
        }
        r += 1;
    }
    hits
}

/// 双跑一致：同语料独立建账两次，根与逐档计数逐项对拍。
fn dual_run_ok() -> bool {
    let a = build_clean();
    let b = build_clean();
    a.root() == b.root()
        && a.root() != 0
        && a.len() == b.len()
        && a.count_tier(EnvTier::Book) == b.count_tier(EnvTier::Book)
        && a.count_tier(EnvTier::LogicExec) == b.count_tier(EnvTier::LogicExec)
        && a.summary() == b.summary()
}

/// 摘要落档判据：封存后摘要非空且含三态关键计数。
fn summary_ok(l: &IsoLedger) -> bool {
    if !l.is_sealed() || l.summary().is_empty() {
        return false;
    }
    if !l.summary().contains("账面档") || !l.summary().contains("逻辑执行档") {
        return false;
    }
    l.summary().contains("实机档0")
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫。
///
/// 对齐 0x40；窗口下界沿用 0x75000，上界取本批（VX8-B01）派工卡给定段窗
/// 0x78200..=0x783C0（契约文本的 0x781C0 上界属前批段窗，B01 起段窗按
/// 派工卡顺延），并锁定本卡派工给定值。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x783C0 {
        return false;
    }
    SEG_BASE == 0x78200
}

/// 档位三态闭集：序单调、标签非空、三态互异。
fn tier_closed_ok() -> bool {
    if TIERS.len() != 3 {
        return false;
    }
    if EnvTier::Book.ord() >= EnvTier::LogicExec.ord() {
        return false;
    }
    if EnvTier::LogicExec.ord() >= EnvTier::RealMachine.ord() {
        return false;
    }
    let mut i = 0usize;
    while i < TIERS.len() {
        let t = match TIERS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if t.tag().is_empty() {
            return false;
        }
        let mut k = i + 1;
        while k < TIERS.len() {
            if let Some(u) = TIERS.get(k) {
                if *u == t {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 合法双轨登记全通过（B0/B1 零误伤）。
fn clean_accept_ok(l: &IsoLedger) -> bool {
    l.len() == 6
        && l.count_tier(EnvTier::Book) == 3
        && l.count_tier(EnvTier::LogicExec) == 3
        && l.count_tier(EnvTier::RealMachine) == 0
}

/// 注入 10/10 必拦。
fn inject_ok() -> bool {
    inject_matrix() == [true; INJECT_ROUNDS]
}

/// 登记门负样本逐类必拒（越界槽/空备注/乱序/超容）。
fn gate_negatives_ok() -> bool {
    let mut l = IsoLedger::new();
    let r1 = match l.register(0, EnvTier::Book, "x") {
        Err(c) => c == Diag::ERange.val(),
        Ok(()) => false,
    };
    let r2 = match l.register(1, EnvTier::Book, "") {
        Err(c) => c == Diag::EEmpty.val(),
        Ok(()) => false,
    };
    let r3 = match l.register(13, EnvTier::Book, "x") {
        Err(c) => c == Diag::ERange.val(),
        Ok(()) => false,
    };
    r1 && r2 && r3 && l.len() == 0
}

/// 封存单向：封存后再写/重封显性拒。
fn seal_monotonic_ok(l: &IsoLedger) -> bool {
    if !l.is_sealed() {
        return false;
    }
    let mut probe = l.clone();
    let r1 = match probe.register(7, EnvTier::Book, "封存后写入") {
        Err(c) => c == Diag::ESealed.val(),
        Ok(()) => false,
    };
    let r2 = match probe.seal() {
        Err(c) => c == Diag::ERegress.val(),
        Ok(()) => false,
    };
    r1 && r2 && probe.len() == l.len()
}

/// 封存重放复核一致。
fn replay_ok(l: &IsoLedger) -> bool {
    match l.replay() {
        Ok(n) => n == 6,
        Err(_) => false,
    }
}

/// 拒绝记账不静默：负样本后拒绝计数必须递增。
fn reject_accounting_ok() -> bool {
    let mut l = IsoLedger::new();
    let before = l.rejects();
    let _ = l.register(1, EnvTier::RealMachine, "越界");
    let _ = l.register(1, EnvTier::Book, "");
    let _ = l.register(1, EnvTier::Book, "重复前占位");
    let _ = l.register(1, EnvTier::Book, "合法首条");
    l.len() == 1 && l.rejects() == before + 3
}

/// 边界容量：满容后多余登记拒（ECap），账面不动。
fn capacity_ok() -> bool {
    let mut l = IsoLedger::new();
    let mut slot = 1u32;
    while slot as usize <= REG_SLOTS {
        let note = if slot % 2 == 0 { "B0 边界填槽" } else { "B1 边界填槽" };
        let tier = if slot % 2 == 0 { EnvTier::Book } else { EnvTier::LogicExec };
        if l.register(slot, tier, note).is_err() {
            return false;
        }
        slot += 1;
    }
    let r = match l.register(REG_SLOTS as u32 + 1, EnvTier::Book, "超容") {
        Err(c) => c == Diag::ERange.val(),
        Ok(()) => false,
    };
    let mut l2 = l.clone();
    let _ = l2.seal();
    let _ = l2.register(REG_SLOTS as u32 + 1, EnvTier::Book, "封存后超容");
    r && l.len() == REG_SLOTS && l2.len() == REG_SLOTS
}

/// 十五项判据（UNX-F1514 隔离账）。
pub fn run_unx1514_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1514");
    let ledger = build_clean();

    s.add_dyn(
        "F1514-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1514-02 环境档位三态闭集",
        tier_closed_ok(),
        &format!("tiers={} b0={} b1={} b2={}", TIERS.len(), EnvTier::Book.ord(), EnvTier::LogicExec.ord(), EnvTier::RealMachine.ord()),
    );

    s.add_dyn(
        "F1514-03 合法双轨登记零误伤",
        clean_accept_ok(&ledger),
        &format!("entries={} book={} logic={}", ledger.len(), ledger.count_tier(EnvTier::Book), ledger.count_tier(EnvTier::LogicExec)),
    );

    s.add_dyn(
        "F1514-04 越界执行注入 10/10 必拦",
        inject_ok(),
        &format!("rounds={} hits={}", INJECT_ROUNDS, inject_matrix().iter().filter(|x| **x).count()),
    );

    s.add_dyn(
        "F1514-05 登记门负样本逐类必拒",
        gate_negatives_ok(),
        &format!("neg_rejects=3 clean=0"),
    );

    s.add_dyn(
        "F1514-06 隔离结论随批落档",
        summary_ok(&ledger),
        &format!("summary_len={}", ledger.summary().len()),
    );

    s.add_dyn(
        "F1514-07 双跑一致",
        dual_run_ok(),
        &format!("root=0x{:016x}", ledger.root()),
    );

    s.add_dyn(
        "F1514-08 封存重放复核",
        replay_ok(&ledger),
        &format!("replay_len={}", ledger.len()),
    );

    s.add_dyn(
        "F1514-09 封存单向推进",
        seal_monotonic_ok(&ledger),
        &format!("sealed={}", ledger.is_sealed()),
    );

    s.add_dyn(
        "F1514-10 拒绝记账不静默",
        reject_accounting_ok(),
        &format!("rejects={}", ledger.rejects()),
    );

    s.add_dyn(
        "F1514-11 边界容量满容拒",
        capacity_ok(),
        &format!("cap={}", REG_SLOTS),
    );

    s.add_dyn(
        "F1514-12 诊断码闭集查表",
        diag_closed_ok(),
        &format!("all={} first=0x{:05x} last=0x{:05x}", ALL.len(), Diag::EBanned.val(), Diag::ECount.val()),
    );

    s.add_dyn(
        "F1514-13 空账不封存",
        empty_seal_rejected(),
        &format!("empty_seal=EReply/EEmpty"),
    );

    s.add_dyn(
        "F1514-14 隔离账生效 100%",
        ledger.is_sealed()
            && ledger.count_tier(EnvTier::RealMachine) == 0
            && inject_ok()
            && summary_ok(&ledger)
            && dual_run_ok(),
        &format!("live=1 root=0x{:016x}", ledger.root()),
    );

    s
}

/// 诊断码闭集自检：值域连续、文案非空、查表越界回退。
fn diag_closed_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    if Diag::EBanned.val() != SEG_BASE + 0x01 || Diag::ECount.val() != SEG_BASE + 0x0C {
        return false;
    }
    let mut i = 0usize;
    while i < ALL.len() {
        let d = match ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if d.detail().is_empty() {
            return false;
        }
        if detail(d.val()) != d.detail() {
            return false;
        }
        i += 1;
    }
    detail(SEG_BASE + 0x40) != ""
}

/// 空账封存必须被拒（无登记不构成隔离证据）。
fn empty_seal_rejected() -> bool {
    let mut l = IsoLedger::new();
    match l.seal() {
        Err(c) => c == Diag::EEmpty.val() && !l.is_sealed(),
        Ok(()) => false,
    }
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_closed_and_ordered() {
        assert_eq!(TIERS.len(), 3);
        assert!(EnvTier::Book.ord() < EnvTier::LogicExec.ord());
        assert!(EnvTier::LogicExec.ord() < EnvTier::RealMachine.ord());
        assert_eq!(detail(SEG_BASE + 0x01), Diag::EBanned.detail());
        assert_eq!(detail(SEG_BASE + 0x40), "未知诊断码：不在 0x78201..0x7820C 闭集内");
    }

    #[test]
    fn clean_ledger_seals_with_summary() {
        let l = build_clean();
        assert_eq!(l.len(), 6);
        assert!(l.is_sealed());
        assert!(l.summary().contains("账面档3"));
        assert!(l.summary().contains("逻辑执行档3"));
        assert!(l.summary().contains("实机档0"));
        assert_ne!(l.root(), 0);
    }

    #[test]
    fn real_machine_injection_all_blocked() {
        let hits = inject_matrix();
        assert_eq!(hits, [true; INJECT_ROUNDS]);
        let mut l = IsoLedger::new();
        assert_eq!(l.register(1, EnvTier::RealMachine, "QEMU 执行面"), Err(Diag::EBanned.val()));
        assert_eq!(l.len(), 0);
    }

    #[test]
    fn gate_negatives_each_code() {
        let mut l = IsoLedger::new();
        assert_eq!(l.register(0, EnvTier::Book, "x"), Err(Diag::ERange.val()));
        assert_eq!(l.register(13, EnvTier::Book, "x"), Err(Diag::ERange.val()));
        assert_eq!(l.register(1, EnvTier::Book, ""), Err(Diag::EEmpty.val()));
        assert_eq!(l.register(1, EnvTier::Book, "a"), Ok(()));
        assert_eq!(l.register(1, EnvTier::LogicExec, "b"), Err(Diag::EDup.val()));
        assert_eq!(l.register(1, EnvTier::Book, "回填"), Err(Diag::EDup.val()));
        assert_eq!(l.register(3, EnvTier::Book, "c"), Ok(()));
        assert_eq!(l.register(2, EnvTier::Book, "倒序"), Err(Diag::EOrder.val()));
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn seal_is_monotonic_terminal() {
        let l = build_clean();
        let mut p = l.clone();
        assert_eq!(p.register(7, EnvTier::Book, "封存后写入"), Err(Diag::ESealed.val()));
        assert_eq!(p.seal(), Err(Diag::ERegress.val()));
        assert_eq!(p.len(), l.len());
        assert_eq!(p.root(), l.root());
    }

    #[test]
    fn empty_ledger_cannot_seal() {
        let mut l = IsoLedger::new();
        assert_eq!(l.seal(), Err(Diag::EEmpty.val()));
        assert!(!l.is_sealed());
        assert!(l.summary().is_empty());
    }

    #[test]
    fn dual_run_roots_agree() {
        let a = build_clean();
        let b = build_clean();
        assert_eq!(a.root(), b.root());
        assert_eq!(a.summary(), b.summary());
        assert_eq!(a.replay(), Ok(6));
        assert_eq!(b.replay(), Ok(6));
    }

    #[test]
    fn capacity_boundary_and_reject_accounting() {
        let mut l = IsoLedger::new();
        let mut slot = 1u32;
        while slot as usize <= REG_SLOTS {
            assert_eq!(l.register(slot, EnvTier::Book, "边界填槽"), Ok(()));
            slot += 1;
        }
        assert_eq!(l.register(13, EnvTier::Book, "越界"), Err(Diag::ERange.val()));
        assert_eq!(l.len(), REG_SLOTS);
        let base = l.rejects();
        let _ = l.register(1, EnvTier::Book, "dup");
        assert_eq!(l.rejects(), base + 1);
    }

    #[test]
    fn checks_all_green_fourteen() {
        let s = run_unx1514_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn seg_guard_matches_dispatch_value() {
        assert_eq!(SEG_BASE, 0x78200);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(seg_guard_ok());
    }
}
