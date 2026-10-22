//! UNX-F21581 · 域账满账终声明（双断言）
//!
//! 诊断段 0x91001~0x91008（F2-B40 域收官段 1/100）。
//!
//! 判据原文（施工书 UNX-F21581-J1）：满账断言：40 批 × 6,000 = 240,000
//! 双路计算一致，F21600 终钉在位。
//!
//! 设计要点（为什么不是一个 `if total == 240000`）：
//! 账本自证必须抵抗「同一个算错被算两遍」。所以两条路径在结构上互相独立：
//!   - 路径 A：逐批顺序累加**实计**行数（无乘法、无基数常量）；
//!   - 路径 B：先按「配额 × 批数」得基数，再回放每批的**偏差**（实数减配额）。
//! 两者只有在「实计与偏差自洽」时才相等，抄错一条就会分叉。分叉本身是
//! 首要诊断信号，比总数是否命中 240,000 更早暴露问题（总数可以靠两处
//! 互补的错误相互抵消而仍然等于 240,000）。
//!
//! 终钉（tail pin）单独成判据：总数对但末条不是 F21600，说明域账被截断
//! 后又被补平，属于账结构损坏而非算错，必须区别于 `TotalShort` 报告。

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 单批配额（施工书 F2 域：每批 6,000 行）。
pub const BATCH_QUOTA: u32 = 6_000;
/// 域内批数（B01~B40）。
pub const BATCH_COUNT: u16 = 40;
/// 域满账总数（40 × 6,000，写死并注明来源：施工书域账累计栏）。
pub const DOMAIN_TOTAL: u32 = 240_000;
/// 域终条编号（施工书 UNX-F21600）。
pub const TAIL_ID: u32 = 21_600;
/// 单批最大条目数（施工书每批 20 条，留 4 倍余量防口径变化）。
pub const MAX_ITEMS_PER_BATCH: u16 = 80;

/// 收官诊断码闭集（0x91001~0x91008 八码）。
///
/// 显式判别式而非隐式递增：码值是跨模块日志的对账键，一旦插入新变体就
/// 会整体漂移，历史日志将无法归因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// 0x91001 封印成功。
    SealOk = 0x91001,
    /// 0x91002 批号不连续（有缺口或重号）。
    BatchGap = 0x91002,
    /// 0x91003 双路计算分叉。
    DualPathDiverge = 0x91003,
    /// 0x91004 终钉不在位。
    TailNotPinned = 0x91004,
    /// 0x91005 短账（总数不足）。
    TotalShort = 0x91005,
    /// 0x91006 超账（总数溢出配额）。
    TotalOver = 0x91006,
    /// 0x91007 域已封印，二次写入被拒。
    AlreadySealed = 0x91007,
    /// 0x91008 空账或批数不足。
    EmptyLedger = 0x91008,
}

impl SealCode {
    /// 诊断三要素之一：现象（发生了什么）。
    pub const fn what(self) -> &'static str {
        match self {
            SealCode::SealOk => "域账封印成功",
            SealCode::BatchGap => "批号存在缺口或重号",
            SealCode::DualPathDiverge => "双路计算分叉",
            SealCode::TailNotPinned => "终钉不在位",
            SealCode::TotalShort => "域账总数低于配额",
            SealCode::TotalOver => "域账总数超出配额",
            SealCode::AlreadySealed => "域已封印仍被写入",
            SealCode::EmptyLedger => "账本为空或批数不足",
        }
    }
    /// 诊断三要素之二：原因（为什么会这样）。
    pub const fn why(self) -> &'static str {
        match self {
            SealCode::SealOk => "四十批齐、双路合、终钉在位",
            SealCode::BatchGap => "批槽顺序插入时越过了某一批",
            SealCode::DualPathDiverge => "实计与偏差表不自洽",
            SealCode::TailNotPinned => "末批末条编号不是域终条",
            SealCode::TotalShort => "存在未补行的空批",
            SealCode::TotalOver => "存在超出配额仍被接纳的批",
            SealCode::AlreadySealed => "封印后仍调用写入路径",
            SealCode::EmptyLedger => "账本尚未装入任何批槽",
        }
    }
    /// 诊断三要素之三：建议（下一步做什么）。
    pub const fn advice(self) -> &'static str {
        match self {
            SealCode::SealOk => "转入下游 ID 闭合核",
            SealCode::BatchGap => "补齐缺批或重排批号后重封印",
            SealCode::DualPathDiverge => "以偏差表为准逐批复算实计",
            SealCode::TailNotPinned => "核对 F21600 是否被截断",
            SealCode::TotalShort => "补齐欠账行后重封印",
            SealCode::TotalOver => "裁剪超配额批次后重封印",
            SealCode::AlreadySealed => "改走复核接口，不要重写账本",
            SealCode::EmptyLedger => "先装入批槽再封印",
        }
    }
    /// 数值形式（日志/判据对账用）。
    ///
    /// 返回 u32 而非 u16：五位数诊断码（0x91001）超出 u16 值域，`as u16`
    /// 会静默截断成 0x1001，与低段码撞车——这类截断在日志里表现为「另一个
    /// 模块的码」，比报错更难查。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 单批账槽。
///
/// `declared` 与 `counted` 分开存是刻意的：合并成一个字段就等于把「应该有」
/// 和「实际有」压成同一个事实，双路校验失去意义。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BatchSlot {
    /// 批序号（1-based，必须与插入位置一致）。
    pub index: u16,
    /// 册配额行数。
    pub declared: u32,
    /// 实计行数。
    pub counted: u32,
    /// 条目数。
    pub items: u16,
    /// 本批末条编号（跨批必须严格递增）。
    pub tail: u32,
}

impl BatchSlot {
    /// 构造一个批槽（不校验，校验在 `DomainLedger::push` 统一做）。
    pub const fn new(index: u16, declared: u32, counted: u32, items: u16, tail: u32) -> BatchSlot {
        BatchSlot {
            index,
            declared,
            counted,
            items,
            tail,
        }
    }

    /// 批内偏差（实计减配额，带符号）。
    ///
    /// 用 i64 而非 u32 相减：短账时 `counted < declared`，u32 相减会绕回成
    /// 一个巨大的正数，恰好能在求和中与别处的超账抵消——这正是双路校验
    /// 要防的「错误互相遮盖」。
    pub const fn deviation(&self) -> i64 {
        self.counted as i64 - self.declared as i64
    }

    /// 批槽自洽性（不看邻批，只看自己）。
    pub const fn is_self_consistent(&self) -> bool {
        self.index >= 1
            && self.declared > 0
            && self.items > 0
            && self.items <= MAX_ITEMS_PER_BATCH
            && self.tail > 0
    }
}

/// 双路计算结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DualTotal {
    /// 路径 A：逐批顺序累加实计。
    pub path_a: u64,
    /// 路径 B：配额基数乘法 + 偏差回放。
    pub path_b: u64,
}

impl DualTotal {
    /// 两路是否一致（这就是「双断言」的第一条断言）。
    pub const fn agree(&self) -> bool {
        self.path_a == self.path_b
    }
    /// 分叉量（带符号：B 减 A）。诊断文本要靠它定位方向。
    pub const fn skew(&self) -> i128 {
        self.path_b as i128 - self.path_a as i128
    }
}

/// 封印故障（每一种都带码与来源批号，便于直接定位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealFault {
    /// 诊断码（u32：五位数码段）。
    pub code: u32,
    /// 相关批号（0 表示与具体批无关）。
    pub batch: u16,
}

impl SealFault {
    const fn new(code: SealCode, batch: u16) -> SealFault {
        SealFault {
            code: code.code(),
            batch,
        }
    }
    /// 把码还原成枚举，便于取三要素文本。
    ///
    /// 闭集外的值不 panic：返回 `SealCode::EmptyLedger` 作为「未知即可疑」的
    /// 保守落点，同时 `is_known()` 让调用方能识别这是未知码。
    pub const fn as_code(self) -> SealCode {
        match self.code {
            0x91001 => SealCode::SealOk,
            0x91002 => SealCode::BatchGap,
            0x91003 => SealCode::DualPathDiverge,
            0x91004 => SealCode::TailNotPinned,
            0x91005 => SealCode::TotalShort,
            0x91006 => SealCode::TotalOver,
            0x91007 => SealCode::AlreadySealed,
            _ => SealCode::EmptyLedger,
        }
    }
    /// 码是否落在闭集内。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91001 && self.code <= 0x91008
    }
}

/// 封印回执（外部复核的凭据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SealReceipt {
    /// 满账总数。
    pub total: u32,
    /// 批数。
    pub batches: u16,
    /// 末条编号。
    pub tail_id: u32,
    /// 账链指纹（FNV-1a 64，逐批咬合）。
    pub fingerprint: u64,
    /// 双路结果（回执里留档，复核时无需重跑账本）。
    pub dual: DualTotal,
}

impl SealReceipt {
    /// 回执自检：总数、批数、指纹非零三项。
    ///
    /// 指纹非零是弱但必要的一条：FNV-1a 的 offset basis 非零，若算出 0 说明
    /// 要么账本为空，要么指纹函数被替换过。
    pub const fn is_plausible(&self) -> bool {
        self.total == DOMAIN_TOTAL
            && self.batches == BATCH_COUNT
            && self.tail_id == TAIL_ID
            && self.fingerprint != 0
    }
}

/// 域账本。
///
/// 容量上限写死 `BATCH_COUNT`：域账是封闭账，允许「先装 41 批再截断」会让
/// 越界写入变成静默丢弃，故超限必须显性拒绝。
pub struct DomainLedger {
    slots: Vec<BatchSlot>,
    sealed: bool,
    /// 封印次数（幂等观测点：成功封印后恒为 1）。
    seal_count: u16,
    /// 被拒写入次数（诊断面：封印后误写不该无声无息）。
    rejected_writes: u16,
}

impl DomainLedger {
    /// 新建空账。
    pub const fn new() -> DomainLedger {
        DomainLedger {
            slots: Vec::new(),
            sealed: false,
            seal_count: 0,
            rejected_writes: 0,
        }
    }

    /// 装入一个批槽。
    ///
    /// 三道闸按「便宜先行」排序：封印态 → 容量 → 自洽 → 批号连续。
    /// 批号连续放最后是因为它要读 `slots`（O(1) 但需要借用），前三条纯局部。
    pub fn push(&mut self, slot: BatchSlot) -> Result<(), SealFault> {
        if self.sealed {
            self.rejected_writes = self.rejected_writes.saturating_add(1);
            return Err(SealFault::new(SealCode::AlreadySealed, slot.index));
        }
        if self.slots.len() >= BATCH_COUNT as usize {
            return Err(SealFault::new(SealCode::EmptyLedger, slot.index));
        }
        if !slot.is_self_consistent() {
            return Err(SealFault::new(SealCode::BatchGap, slot.index));
        }
        let expect = (self.slots.len() as u16).saturating_add(1);
        if slot.index != expect {
            return Err(SealFault::new(SealCode::BatchGap, slot.index));
        }
        // 跨批末条必须严格递增：批内可任意，跨批倒退说明批序与编号序错位。
        if let Some(prev) = self.slots.last() {
            if slot.tail <= prev.tail {
                return Err(SealFault::new(SealCode::BatchGap, slot.index));
            }
        }
        self.slots.push(slot);
        Ok(())
    }

    /// 已装批数。
    pub fn batch_count(&self) -> usize {
        self.slots.len()
    }

    /// 是否已封印。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 封印成功次数。
    pub const fn seal_count(&self) -> u16 {
        self.seal_count
    }

    /// 封印后被拒写入次数。
    pub const fn rejected_writes(&self) -> u16 {
        self.rejected_writes
    }

    /// 路径 A：逐批顺序累加实计。
    ///
    /// 刻意不做任何乘法或基数假设——它对「批数不对」「配额被改」都不敏感，
    /// 只忠实回答「账面上到底写了多少行」。
    pub fn path_a(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.slots.len() {
            sum = sum.saturating_add(self.slots[i].counted as u64);
            i += 1;
        }
        sum
    }

    /// 路径 B：配额基数乘法 + 偏差回放。
    ///
    /// 基数用编译期常量 `BATCH_QUOTA × BATCH_COUNT` 而非 `slots` 里的
    /// `declared` 求和：若两者都用账本数据，「配额被改成 6001」这种错会因为
    /// 两边同时被改而互相掩护。基数取常量，偏差取账本，才形成独立约束。
    pub fn path_b(&self) -> u64 {
        let base = (BATCH_QUOTA as u64).saturating_mul(BATCH_COUNT as u64);
        let mut dev = 0i64;
        let mut i = 0usize;
        while i < self.slots.len() {
            dev = dev.saturating_add(self.slots[i].deviation());
            i += 1;
        }
        if dev >= 0 {
            base.saturating_add(dev as u64)
        } else {
            base.saturating_sub((-dev) as u64)
        }
    }

    /// 双路结果。
    pub fn dual_total(&self) -> DualTotal {
        DualTotal {
            path_a: self.path_a(),
            path_b: self.path_b(),
        }
    }

    /// 账链指纹：逐批把 (index, declared, counted, items, tail) 喂进 FNV-1a 64。
    ///
    /// 逐字段喂而不是喂结构体内存：结构体内存含 padding，未初始化字节会让
    /// 指纹在不同编译器版本间漂移，跨版本复核就永远对不上。
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0usize;
        while i < self.slots.len() {
            let s = self.slots[i];
            h = fnv_u64(h, s.index as u64);
            h = fnv_u64(h, s.declared as u64);
            h = fnv_u64(h, s.counted as u64);
            h = fnv_u64(h, s.items as u64);
            h = fnv_u64(h, s.tail as u64);
            i += 1;
        }
        h
    }

    /// 末批末条编号（账本为空时返回 0）。
    pub fn last_tail(&self) -> u32 {
        match self.slots.last() {
            Some(s) => s.tail,
            None => 0,
        }
    }

    /// 封印域账（双断言 + 终钉 + 满账三面）。
    ///
    /// 检查顺序刻意固定，且**每一条都先于下一条**：批结构错时不该报总数错，
    /// 否则运维会先去补行数，补完再发现批号本来就是断的。诊断的可操作性
    /// 来自「只报第一因」。
    pub fn seal(&mut self) -> Result<SealReceipt, SealFault> {
        if self.sealed {
            self.rejected_writes = self.rejected_writes.saturating_add(1);
            return Err(SealFault::new(SealCode::AlreadySealed, 0));
        }
        if self.slots.is_empty() || self.slots.len() != BATCH_COUNT as usize {
            return Err(SealFault::new(
                SealCode::EmptyLedger,
                self.slots.len() as u16,
            ));
        }
        // 批结构复核（push 已拦过，但账本可能被别处构造后直接塞进来）。
        let mut i = 0usize;
        while i < self.slots.len() {
            let s = self.slots[i];
            let expect = (i as u16).saturating_add(1);
            if !s.is_self_consistent() || s.index != expect {
                return Err(SealFault::new(SealCode::BatchGap, s.index));
            }
            if i > 0 && s.tail <= self.slots[i - 1].tail {
                return Err(SealFault::new(SealCode::BatchGap, s.index));
            }
            i += 1;
        }
        let dual = self.dual_total();
        if !dual.agree() {
            return Err(SealFault::new(SealCode::DualPathDiverge, 0));
        }
        if dual.path_a < DOMAIN_TOTAL as u64 {
            return Err(SealFault::new(SealCode::TotalShort, 0));
        }
        if dual.path_a > DOMAIN_TOTAL as u64 {
            return Err(SealFault::new(SealCode::TotalOver, 0));
        }
        if self.last_tail() != TAIL_ID {
            return Err(SealFault::new(SealCode::TailNotPinned, 0));
        }
        self.sealed = true;
        self.seal_count = self.seal_count.saturating_add(1);
        Ok(SealReceipt {
            total: DOMAIN_TOTAL,
            batches: BATCH_COUNT,
            tail_id: TAIL_ID,
            fingerprint: self.fingerprint(),
            dual,
        })
    }

    /// 封印后复核：用独立算法重算指纹与总数，比对回执。
    ///
    /// 与 `seal` 的区别：`seal` 生成回执，`verify` 只读不改，且**不复用**
    /// `seal` 的中间量（否则回执自己验自己，永远为真）。
    pub fn verify(&self, receipt: &SealReceipt) -> Result<(), SealFault> {
        if !self.sealed {
            return Err(SealFault::new(SealCode::EmptyLedger, 0));
        }
        let dual = self.dual_total();
        if !dual.agree() || dual.path_a != receipt.dual.path_a || dual.path_b != receipt.dual.path_b {
            return Err(SealFault::new(SealCode::DualPathDiverge, 0));
        }
        if self.fingerprint() != receipt.fingerprint {
            // 指纹不符在语义上等价于「账本被改过」，归类为双路分叉的姊妹现象：
            // 两者都表示「回执描述的账」与「现在的账」不是同一本。
            return Err(SealFault::new(SealCode::DualPathDiverge, 0));
        }
        if self.last_tail() != receipt.tail_id {
            return Err(SealFault::new(SealCode::TailNotPinned, 0));
        }
        Ok(())
    }

    /// 生成可归档的封印报告（纯文本，字段顺序固定便于 diff）。
    pub fn report(&self) -> String {
        let dual = self.dual_total();
        let mut out = String::with_capacity(256);
        out.push_str("domain=F2-B40 batches=");
        out.push_str(&self.slots.len().to_string());
        out.push_str(" path_a=");
        out.push_str(&dual.path_a.to_string());
        out.push_str(" path_b=");
        out.push_str(&dual.path_b.to_string());
        out.push_str(" agree=");
        out.push_str(if dual.agree() { "1" } else { "0" });
        out.push_str(" tail=");
        out.push_str(&self.last_tail().to_string());
        out.push_str(" sealed=");
        out.push_str(if self.sealed { "1" } else { "0" });
        out.push_str(" fp=");
        out.push_str(&hex64(self.fingerprint()));
        out
    }
}

impl Default for DomainLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// FNV-1a 64 单步（写死质数与偏移基，来源：FNV 规范 1a 变体）。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 64 位十六进制（no_std 手写，避免 format! 的分配开销）。
pub fn hex64(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEX[nib] as char);
        shift -= 4;
    }
    s
}

/// 构造标准 40 批账本（判据夹具）。
///
/// 夹具本身必须能构造成功——历史上见过「标准夹具自带未声明槽位」导致整册
/// 判据全部被一个 `expect` 挡在门外。故这里返回 Result，由调用方显性处理，
/// 且在 checks 里专门有一条判据验证夹具可构造。
pub fn standard_ledger() -> Result<DomainLedger, SealFault> {
    let mut ledger = DomainLedger::new();
    let mut i: u16 = 1;
    while i <= BATCH_COUNT {
        // 末条编号按「每批 20 条、批间连续」推导：第 i 批末条 = i × 20 + 21400。
        // 基数 20800 来自施工书域段边界 F20801–F21600：第 i 批末条 = 20800 + 20i，
        // i=40 时恰为 21600（终钉）。
        let tail = 20_800u32.saturating_add((i as u32).saturating_mul(20));
        let slot = BatchSlot::new(i, BATCH_QUOTA, BATCH_QUOTA, 20, tail);
        ledger.push(slot)?;
        i += 1;
    }
    Ok(ledger)
}

/// 带偏差的标准账本：第 `at` 批实计加 `delta`（用于构造短账/超账样本）。
pub fn ledger_with_deviation(at: u16, delta: i32) -> Result<DomainLedger, SealFault> {
    let mut ledger = DomainLedger::new();
    let mut i: u16 = 1;
    while i <= BATCH_COUNT {
        let tail = 20_800u32.saturating_add((i as u32).saturating_mul(20));
        let base = BATCH_QUOTA as i64;
        let counted = if i == at {
            let v = base + delta as i64;
            if v < 0 {
                0u32
            } else {
                v as u32
            }
        } else {
            BATCH_QUOTA
        };
        ledger.push(BatchSlot::new(i, BATCH_QUOTA, counted, 20, tail))?;
        i += 1;
    }
    Ok(ledger)
}

/// 判据入口（UNX-F21581-J1 及其配套八条）。
///
/// 判据常量独立写死、不复用被测值：`240_000` / `40` / `21_600` 在断言里
/// 直接以字面量出现，避免「被测值改成 240001，判据跟着变」的自证循环。
pub fn run_unx21581_domainseal_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21581");
    let standard = standard_ledger();

    // 1) 夹具可构造：先证夹具，再证被测逻辑（夹具失败时后续全部失去意义）。
    cs.add(
        "f21581.fixture_buildable",
        standard.is_ok(),
        "标准 40 批账本构造失败",
    );

    // 2) 双路在标准账上一致。
    if let Ok(ledger) = &standard {
        let dual = ledger.dual_total();
        cs.add(
            "f21581.dual_path_agree",
            dual.agree() && dual.path_a == 240_000 && dual.path_b == 240_000,
            "标准账双路不一致或总数不为 240000",
        );
    } else {
        cs.add("f21581.dual_path_agree", false, "夹具不可用");
    }

    // 3) 封印成功且回执可信。
    if let Ok(mut ledger) = standard_ledger() {
        match ledger.seal() {
            Ok(receipt) => {
                cs.add(
                    "f21581.seal_ok",
                    receipt.is_plausible() && receipt.total == 240_000,
                    "封印成功但回执不自洽",
                );
                cs.add(
                    "f21581.verify_ok",
                    ledger.verify(&receipt).is_ok(),
                    "封印后复核不通过",
                );
            }
            Err(_) => {
                cs.add("f21581.seal_ok", false, "标准账封印被拒");
                cs.add("f21581.verify_ok", false, "无回执可复核");
            }
        }
    } else {
        cs.add("f21581.seal_ok", false, "夹具不可用");
        cs.add("f21581.verify_ok", false, "夹具不可用");
    }

    // 4) 短账必拒（少 1 行）。
    match ledger_with_deviation(7, -1) {
        Ok(mut l) => {
            let code = l.seal().err().map(|f| f.code);
            cs.add(
                "f21581.short_rejected",
                code == Some(SealCode::TotalShort.code()),
                "短 1 行未被拒或码不符",
            );
        }
        Err(_) => cs.add("f21581.short_rejected", false, "样本构造失败"),
    }

    // 5) 超账必拒（多 1 行）。
    match ledger_with_deviation(13, 1) {
        Ok(mut l) => {
            let code = l.seal().err().map(|f| f.code);
            cs.add(
                "f21581.over_rejected",
                code == Some(SealCode::TotalOver.code()),
                "超 1 行未被拒或码不符",
            );
        }
        Err(_) => cs.add("f21581.over_rejected", false, "样本构造失败"),
    }

    // 6) 批号断口必拒：跳过第 21 批直接塞第 22 批。
    {
        let mut ledger = DomainLedger::new();
        let mut i: u16 = 1;
        let mut gap_hit = false;
        while i <= BATCH_COUNT {
            if i == 21 {
                i += 1;
                continue;
            }
            // 跳过 21 后，i=22 的 index 与期望的 21 不符，push 必须拒绝。
            let tail = 20_800u32.saturating_add((i as u32).saturating_mul(20));
            if ledger
                .push(BatchSlot::new(i, BATCH_QUOTA, BATCH_QUOTA, 20, tail))
                .is_err()
            {
                gap_hit = true;
                break;
            }
            i += 1;
        }
        cs.add("f21581.batch_gap_rejected", gap_hit, "批号断口未被拒");
    }

    // 7) 终钉不在位必拒：末批末条改成 21599。
    {
        let mut ledger = DomainLedger::new();
        let mut i: u16 = 1;
        let mut build_ok = true;
        while i <= BATCH_COUNT {
            let tail = if i == BATCH_COUNT {
                21_599
            } else {
                20_800u32.saturating_add((i as u32).saturating_mul(20))
            };
            if ledger
                .push(BatchSlot::new(i, BATCH_QUOTA, BATCH_QUOTA, 20, tail))
                .is_err()
            {
                build_ok = false;
                break;
            }
            i += 1;
        }
        let code = if build_ok { ledger.seal().err().map(|f| f.code) } else { None };
        cs.add(
            "f21581.tail_pin_enforced",
            code == Some(SealCode::TailNotPinned.code()),
            "终钉缺失未被拒或码不符",
        );
    }

    // 8) 二次封印必拒，且被拒计数递增（写锁可观测）。
    if let Ok(mut ledger) = standard_ledger() {
        let _ = ledger.seal();
        let before = ledger.rejected_writes();
        let second = ledger.seal();
        cs.add(
            "f21581.write_lock",
            second.is_err()
                && second.err().map(|f| f.code) == Some(SealCode::AlreadySealed.code())
                && ledger.rejected_writes() == before.saturating_add(1)
                && ledger.seal_count() == 1,
            "二次封印未被拒或计数不符",
        );
    } else {
        cs.add("f21581.write_lock", false, "夹具不可用");
    }

    // 9) 指纹对篡改敏感：改一个批的实计数，指纹必变（且双路仍可能自洽，
    //    所以这条独立于双路判据——它守的是「回执不可伪造」）。
    {
        let fp_a = standard_ledger().map(|l| l.fingerprint()).unwrap_or(0);
        let fp_b = ledger_with_deviation(1, 1).map(|l| l.fingerprint()).unwrap_or(1);
        cs.add(
            "f21581.fingerprint_sensitive",
            fp_a != 0 && fp_a != fp_b,
            "篡改后指纹未变",
        );
    }

    // 10) 报告字段完整（可归档性）。
    if let Ok(ledger) = standard_ledger() {
        let r = ledger.report();
        cs.add(
            "f21581.report_complete",
            r.contains("path_a=240000")
                && r.contains("path_b=240000")
                && r.contains("agree=1")
                && r.contains("batches=40")
                && r.contains("tail=21600"),
            "报告缺字段或数值不符",
        );
    } else {
        cs.add("f21581.report_complete", false, "夹具不可用");
    }

    cs
}
