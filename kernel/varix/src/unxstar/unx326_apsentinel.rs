//! unx326_apsentinel ↔ UNX-F0326 AP 到达哨兵与超时账
//!
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B10）：UNX-F0326 · AP 到达哨兵与
//! 超时账 · 判据：`UNX-F0326-J1 哨兵原子置位语义正确，超时（200ms/AP）
//! 检出与分类账齐`
//!
//! 设计要点（围绕 INIT/SIPI 汇合路径上"AP 到底有没有到"的真实语义）：
//! 1. **哨兵是一个字，不是标志集合**：每个 AP 有一个到达哨兵字
//!    （sentinel word），语义只有三态——`未达/0x0`、`到达/0xA5A5_5A5A`、
//!    `作废/0xDEAD_BEEF`。哨兵**只能被 AP 自己置位**（谁置位必须留
//!    痕：置位者 AP 号 + 逻辑 tick），因为"到没到"这件事只有 AP 本人
//!    知道；核心代置等价于**伪造到达**，那会让整个汇合栅栏提前放行
//!    ——比超时严重得多（放行一个没到的 AP = 拿着未初始化状态跑代码）。
//! 2. **原子置位语义**：置位必须是一次读-改-写循环内的
//!    **test-and-set**，其可观察语义是"恰好一次生效"：并发重复置位，
//!    第二次必须返回 `AlreadySet` 而不是静默成功——静默成功会让重复
//!    到达被当成两次独立到达，破坏到达计数守恒。
//! 3. **200ms/AP 超时账**：汇合栅栏的等待预算不是固定 200ms，而是
//!    **200ms × 已置位 AP 数**（分批到达要续期，未到的不续期）。这是
//!    真实硬件里NMI 到达窗口的算法。超时分三类入账：
//!    （a）`NoSIPI`——连 SIPI 序列都没发出就超时（上游没动作）；
//!    （b）`SipiSentNoArrive`——SIPI 已发但哨兵未置位（AP 没响应，
//!    典型是 AP 核未从复位态出来）；
//!    （c）`LateArrive`——超时后才置位（晚到，需立案而不是静默接受，
//!    因为它意味着当初的超时分类可能是错的）。
//! 4. **超时分类必须可复算**：分类账记的是"超时那一刻的快照"，事后
//!    AP 状态变化不得改写历史行（append-only）。故每行自带
//!    `at_sipisent: bool` 与 `at_arrive: bool` 两个当时事实，事后重放
//!    分类器必须得同一结果——这叫**分类幂等**。
//! 5. **预算续期不得越界放大**：续期只在"预算将被耗尽且仍有 AP 在途"
//!    时发生，且每轮续期额度有上限（`MAX_RENEW`），防止一个迟迟不
//!    到的 AP 把栅栏永远吊着——那是"活锁式等待"，比超时更坏。
//!    超上限即 `RenewExhausted` 立案。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const V326_VERSION: &str = "v326-apsentinel-v1";

/// 诊断段基址（0x6600 段 · 本模块独占）。
pub const V326_SEG_BASE: u16 = 0x6600;

/// 每 AP 超时预算（逻辑 tick 口径 · 200_000_000ns = 200ms）。
pub const AP_BUDGET_TICKS: u64 = 200_000;

/// AP 池容量。
pub const AP_CAP: usize = 16;

/// 超时账容量（append-only 定容）。
pub const LEDGER_CAP: usize = 64;

/// 单 AP 最大续期轮数。
pub const MAX_RENEW: u32 = 4;

/// 哨兵：未达。
pub const SENT_UNSET: u32 = 0x0000_0000;
/// 哨兵：到达（魔数· AP 自置）。
pub const SENT_ARRIVED: u32 = 0xA5A5_5A5A;
/// 哨兵：作废（AP 被隔离/放弃）。
pub const SENT_VOID: u32 = 0xDEAD_BEEF;

/// --------------------------------------------------------------------------
// 诊断码（十二码 · 段内连续）
// --------------------------------------------------------------------------

/// AP 到达哨兵诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V326Code {
    /// 0x6600 AP 池越界。
    ApRange,
    /// 0x6601 哨兵被非 AP 身份置位（伪造到达）。
    SentinelForged,
    /// 0x6602 哨兵重复置位（到达计数将破守恒）。
    AlreadySet,
    /// 0x6603 哨兵魔数非法（写入非闭集值）。
    SentinelBad,
    /// 0x6604 逻辑时钟回退（超时账不可信）。
    ClockBack,
    /// 0x6605 汇合栅栏未开即查超时（顺序错）。
    NotOpen,
    /// 0x6606 超时账已满（显性拒，不抹旧账）。
    LedgerFull,
    /// 0x6607 续期次数越上限（活锁式等待）。
    RenewExhausted,
    /// 0x6608 未到 AP 却申请续期（预算无意义）。
    RenewNoPending,
    /// 0x6609 到达计数与哨兵实况不符。
    CountDrift,
    /// 0x660A 超时分类复算不幂等（历史行被改写）。
    ClassNotIdem,
    /// 0x660B 栅栏在全员到达前放行（提前放行 = 带未初始化状态跑）。
    EarlyRelease,
}

impl V326Code {
    /// 码值。
    pub fn val(self) -> u16 {
        V326_SEG_BASE
            + match self {
                V326Code::ApRange => 0x0,
                V326Code::SentinelForged => 0x1,
                V326Code::AlreadySet => 0x2,
                V326Code::SentinelBad => 0x3,
                V326Code::ClockBack => 0x4,
                V326Code::NotOpen => 0x5,
                V326Code::LedgerFull => 0x6,
                V326Code::RenewExhausted => 0x7,
                V326Code::RenewNoPending => 0x8,
                V326Code::CountDrift => 0x9,
                V326Code::ClassNotIdem => 0xA,
                V326Code::EarlyRelease => 0xB,
            }
    }

    /// 全部码（顺序即码序）。
    pub const ALL: [V326Code; 12] = [
        V326Code::ApRange,
        V326Code::SentinelForged,
        V326Code::AlreadySet,
        V326Code::SentinelBad,
        V326Code::ClockBack,
        V326Code::NotOpen,
        V326Code::LedgerFull,
        V326Code::RenewExhausted,
        V326Code::RenewNoPending,
        V326Code::CountDrift,
        V326Code::ClassNotIdem,
        V326Code::EarlyRelease,
    ];

    /// 可操作建议（错误路径必带修复指引）。
    pub fn remedy(self) -> &'static str {
        match self {
            V326Code::ApRange => "AP 号越界：限 AP_CAP 内，先查 ap_id 来源（拓扑派生）",
            V326Code::SentinelForged => "伪造到达：哨兵只准 AP 自置，查是谁代置了 arrive()",
            V326Code::AlreadySet => "重复到达：第二次置位须被拒，否则到达计数守恒破",
            V326Code::SentinelBad => "哨兵魔数非法：只准 0 / 0xA5A55A5A / 0xDEADBEEF",
            V326Code::ClockBack => "逻辑时钟回退：超时账基于 tick 差，回退即账不可信",
            V326Code::NotOpen => "栅栏未开就查超时：先 open_fence() 再 sweep()",
            V326Code::LedgerFull => "超时账满：先 seal 归档再续写（append-only不可抹）",
            V326Code::RenewExhausted => "续期越上限：该AP 已吊死 MAX_RENEW 轮，强制入超时账",
            V326Code::RenewNoPending => "无在途 AP 却续期：预算不会被用到，拒绝空转续期",
            V326Code::CountDrift => "到达计数漂移：重数哨兵实况与计数账，定位漏置/多置",
            V326Code::ClassNotIdem => "分类复算不幂等：历史行当时事实被改，append-only 破了",
            V326Code::EarlyRelease => "提前放行：栅栏未齐就release，等于拿未初始化状态跑代码",
        }
    }

    /// 一行式描述。
    pub fn line(self) -> String {
        format!("[0x{:04X}] {}", self.val(), self.remedy())
    }
}

// --------------------------------------------------------------------------
// AP 槽
// --------------------------------------------------------------------------

/// AP 槽（哨兵 + 到达痕迹 + SIPI 时刻）。
#[derive(Clone, Debug)]
pub struct ApSlot {
    /// AP 号。
    pub id: u8,
    /// 哨兵字。
    pub sentinel: u32,
    /// 置位者 AP 号（0 = 未置位）。
    pub set_by: u8,
    /// 置位逻辑 tick。
    pub set_at: u64,
    /// SIPI 发出逻辑 tick（0 = 未发）。
    pub sipi_at: u64,
    /// 续期轮数。
    pub renews: u32,
    /// 已入超时账（防重复入账）。
    pub in_ledger: bool,
    /// 迟到达标记（超时后才置位）。
    pub late: bool,
}

impl ApSlot {
    /// 造空槽。
    pub fn new(id: u8) -> ApSlot {
        ApSlot {
            id,
            sentinel: SENT_UNSET,
            set_by: 0,
            set_at: 0,
            sipi_at: 0,
            renews: 0,
            in_ledger: false,
            late: false,
        }
    }

    /// 是否已到达。
    pub fn arrived(&self) -> bool {
        self.sentinel == SENT_ARRIVED
    }

    /// 是否作废。
    pub fn voided(&self) -> bool {
        self.sentinel == SENT_VOID
    }

    /// 是否仍在途（未达未作废）。
    pub fn pending(&self) -> bool {
        self.sentinel == SENT_UNSET
    }

    /// 行指纹（FNV-1a · 覆盖全部可观察字段）。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| {
            for b in v.to_le_bytes().iter() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        mix(self.id as u64);
        mix(self.sentinel as u64);
        mix(self.set_by as u64);
        mix(self.set_at);
        mix(self.sipi_at);
        mix(self.renews as u64);
        mix(self.in_ledger as u64);
        mix(self.late as u64);
        h
    }
}

// --------------------------------------------------------------------------
// 超时分类
// --------------------------------------------------------------------------

/// 超时分类（三类 + 正常）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeoutClass {
    /// 未超时。
    None,
    /// 类A：连 SIPI 都没发（上游没动作）。
    NoSipi,
    /// 类 B：SIPI 已发但哨兵未置（AP 没响应）。
    SipiSentNoArrive,
    /// 类 C：超时后才置位（晚到，立案）。
    LateArrive,
}

impl TimeoutClass {
    /// 码值。
    pub fn val(self) -> u8 {
        match self {
            TimeoutClass::None => 0,
            TimeoutClass::NoSipi => 1,
            TimeoutClass::SipiSentNoArrive => 2,
            TimeoutClass::LateArrive => 3,
        }
    }

    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            TimeoutClass::None => "未超时",
            TimeoutClass::NoSipi => "未发SIPI",
            TimeoutClass::SipiSentNoArrive => "已发未达",
            TimeoutClass::LateArrive => "迟到达",
        }
    }

    /// 是否为超时（入账条件）。
    pub fn timed_out(self) -> bool {
        !matches!(self, TimeoutClass::None)
    }
}

/// 超时账行（append-only · 自带当时事实）。
#[derive(Clone, Debug)]
pub struct LedgerRow {
    /// 序号（单调）。
    pub seq: u64,
    /// AP 号。
    pub ap: u8,
    /// 分类。
    pub class: TimeoutClass,
    /// 超时判定时刻。
    pub at: u64,
    /// 判定当时 SIPI 是否已发（当时事实·不可事后改）。
    pub at_sipisent: bool,
    /// 判定当时是否已到达（当时事实）。
    pub at_arrive: bool,
    /// 判定时续期轮数。
    pub at_renews: u32,
    /// 行指纹。
    pub fp: u64,
}

impl LedgerRow {
    /// 造行（当时事实从槽位快照取）。
    fn make(seq: u64, ap: u8, class: TimeoutClass, at: u64, s: &ApSlot) -> LedgerRow {
        LedgerRow {
            seq,
            ap,
            class,
            at,
            at_sipisent: s.sipi_at != 0,
            at_arrive: s.arrived(),
            at_renews: s.renews,
            fp: 0,
        }
    }

    /// 重算行指纹。
    fn compute_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| {
            for b in v.to_le_bytes().iter() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        mix(self.seq);
        mix(self.ap as u64);
        mix(self.class.val() as u64);
        mix(self.at);
        mix(self.at_sipisent as u64);
        mix(self.at_arrive as u64);
        mix(self.at_renews as u64);
        h
    }

    /// 指纹自检（防事后改写）。
    pub fn fp_intact(&self) -> bool {
        self.fp == self.compute_fp()
    }

    /// 事后复算分类（只依赖当时事实 · 必须与 `class` 一致）。
    pub fn reclassify(&self) -> TimeoutClass {
        if self.at_arrive {
            TimeoutClass::LateArrive
        } else if self.at_sipisent {
            TimeoutClass::SipiSentNoArrive
        } else {
            TimeoutClass::NoSipi
        }
    }
}

// --------------------------------------------------------------------------
// 哨兵机
// --------------------------------------------------------------------------

/// AP 到达哨兵机。
#[derive(Clone, Debug)]
pub struct Sentinel {
    /// AP 槽表。
    pub aps: Vec<ApSlot>,
    /// 栅栏是否已开。
    pub open: bool,
    /// 当前逻辑 tick。
    pub now: u64,
    /// 超时账（定容 append-only）。
    pub book: Vec<LedgerRow>,
    /// 续期总额度。
    pub renew_budget: u32,
    /// 统计：到达次数（守恒口径）。
    pub n_arrive: u64,
    /// 统计：置位被拒次数（重复）。
    pub n_dup: u64,
    /// 统计：伪造就位次数。
    pub n_forge: u64,
    /// 统计：作废次数。
    pub n_void: u64,
    /// 统计：放行次数。
    pub n_release: u32,
    /// 立案计数（12 格）。
    pub stats: [u64; 12],
}

impl Sentinel {
    /// 造哨兵机（`n` 个 AP）。
    pub fn new(n: usize) -> Sentinel {
        let mut aps: Vec<ApSlot> = Vec::new();
        let cnt = if n > AP_CAP { AP_CAP } else { n };
        for i in 0..cnt {
            aps.push(ApSlot::new(i as u8));
        }
        Sentinel {
            aps,
            open: false,
            now: 0,
            book: Vec::new(),
            renew_budget: 0,
            n_arrive: 0,
            n_dup: 0,
            n_forge: 0,
            n_void: 0,
            n_release: 0,
            stats: [0; 12],
        }
    }

    /// 立案计数自增。
    fn bump(&mut self, c: V326Code) {
        let i = (c.val() - V326_SEG_BASE) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: V326Code) -> u64 {
        let i = (c.val() - V326_SEG_BASE) as usize;
        self.stats.get(i).copied().unwrap_or(0)
    }

    /// AP 号校验。
    fn check(&mut self, ap: u8) -> Result<usize, V326Code> {
        let i = ap as usize;
        if i >= self.aps.len() {
            self.bump(V326Code::ApRange);
            return Err(V326Code::ApRange);
        }
        Ok(i)
    }

    /// 推进逻辑时钟（单调·回退即拒）。
    pub fn tick(&mut self, to: u64) -> Result<u64, V326Code> {
        if to < self.now {
            self.bump(V326Code::ClockBack);
            return Err(V326Code::ClockBack);
        }
        self.now = to;
        Ok(self.now)
    }

    /// 开栅栏（记录起始 tick）。
    pub fn open_fence(&mut self, at: u64) -> Result<(), V326Code> {
        self.tick(at)?;
        self.open = true;
        Ok(())
    }

    /// 发 SIPI（记录时刻 · 重复发同 AP 幂等返回原时刻）。
    pub fn send_sipi(&mut self, ap: u8, at: u64) -> Result<u64, V326Code> {
        let i = self.check(ap)?;
        self.tick(at)?;
        let s = match self.aps.get_mut(i) {
            Some(s) => s,
            None => return Err(V326Code::ApRange),
        };
        if s.sipi_at == 0 {
            s.sipi_at = at;
        }
        Ok(s.sipi_at)
    }

    /// 哨兵置位（test-and-set · 恰好一次生效）。
    ///
    /// `who` 是自称的置位者 AP 号；必须等于槽自身 AP 号，否则是核心
    /// 或第三方代置 —— 等价伪造到达（`SentinelForged`）。
    pub fn arrive(&mut self, ap: u8, who: u8, at: u64) -> Result<u64, V326Code> {
        let i = self.check(ap)?;
        if who != ap {
            self.bump(V326Code::SentinelForged);
            self.n_forge = self.n_forge.wrapping_add(1);
            return Err(V326Code::SentinelForged);
        }
        self.tick(at)?;
        let s = match self.aps.get_mut(i) {
            Some(s) => s,
            None => return Err(V326Code::ApRange),
        };
        // test-and-set：非 unset 一律拒（含重复到达与作废后到达）。
        if s.sentinel != SENT_UNSET {
            self.bump(V326Code::AlreadySet);
            self.n_dup = self.n_dup.wrapping_add(1);
            return Err(V326Code::AlreadySet);
        }
        s.sentinel = SENT_ARRIVED;
        s.set_by = who;
        s.set_at = at;
        // 迟到达判定：已入超时账后才置位 → late。
        s.late = s.in_ledger;
        self.n_arrive = self.n_arrive.wrapping_add(1);
        Ok(at)
    }

    /// 作废 AP（放弃该 AP · 显式留痕）。
    pub fn void_ap(&mut self, ap: u8, at: u64) -> Result<(), V326Code> {
        let i = self.check(ap)?;
        self.tick(at)?;
        let s = match self.aps.get_mut(i) {
            Some(s) => s,
            None => return Err(V326Code::ApRange),
        };
        if s.sentinel != SENT_UNSET {
            self.bump(V326Code::AlreadySet);
            return Err(V326Code::AlreadySet);
        }
        s.sentinel = SENT_VOID;
        s.set_by = 0;
        s.set_at = at;
        self.n_void = self.n_void.wrapping_add(1);
        Ok(())
    }

    /// 当前预算（200ms × 已置位 AP 数 · 至少 1 份）。
    pub fn budget(&self) -> u64 {
        let arrived = self.aps.iter().filter(|s| s.arrived()).count() as u64;
        let base = if arrived == 0 { 1 } else { arrived };
        AP_BUDGET_TICKS.wrapping_mul(base)
    }

    /// 栅栏起始 tick（首条 SIPI 或开栅栏时刻，取先者）。
    pub fn fence_start(&self) -> u64 {
        let mut start = 0u64;
        for s in self.aps.iter() {
            if s.sipi_at != 0 {
                start = if start == 0 { s.sipi_at } else { start.min(s.sipi_at) };
            }
        }
        if start == 0 {
            self.now
        } else {
            start
        }
    }

    /// 是否预算耗尽（`now - start >= budget`）。
    pub fn budget_exhausted(&self) -> bool {
        self.now.wrapping_sub(self.fence_start()) >= self.budget()
    }

    /// 续期（有在途 AP 且未越上限时才允许）。
    pub fn renew(&mut self) -> Result<u32, V326Code> {
        let pending = self.aps.iter().filter(|s| s.pending()).count();
        if pending == 0 {
            self.bump(V326Code::RenewNoPending);
            return Err(V326Code::RenewNoPending);
        }
        if self.renew_budget >= MAX_RENEW {
            self.bump(V326Code::RenewExhausted);
            return Err(V326Code::RenewExhausted);
        }
        // 续期按在途 AP 计轮（每个在途 AP 各+1 · 上限约束全体）。
        for s in self.aps.iter_mut() {
            if s.pending() && s.renews < MAX_RENEW {
                s.renews = s.renews.wrapping_add(1);
            } else if s.pending() {
                self.bump(V326Code::RenewExhausted);
                return Err(V326Code::RenewExhausted);
            }
        }
        self.renew_budget = self.renew_budget.wrapping_add(1);
        Ok(self.renew_budget)
    }

    /// 扫超时（分类入账 · append-only）。
    pub fn sweep(&mut self) -> Result<u32, V326Code> {
        if !self.open {
            self.bump(V326Code::NotOpen);
            return Err(V326Code::NotOpen);
        }
        if !self.budget_exhausted() {
            return Ok(0);
        }
        let start = self.fence_start();
        let now = self.now;
        let mut added = 0u32;
        for i in 0..self.aps.len() {
            let (pending, already) = match self.aps.get(i) {
                Some(s) => (s.pending(), s.in_ledger),
                None => continue,
            };
            if !pending || already {
                continue;
            }
            // 只有"从 SIPI 发出（或开栅栏）起已超预算"才入账。
            let s = match self.aps.get(i) {
                Some(s) => s,
                None => continue,
            };
            let base = if s.sipi_at != 0 { s.sipi_at } else { start };
            if now.wrapping_sub(base) < AP_BUDGET_TICKS {
                continue;
            }
            if self.book.len() >= LEDGER_CAP {
                self.bump(V326Code::LedgerFull);
                return Err(V326Code::LedgerFull);
            }
            let class = if s.sipi_at == 0 {
                TimeoutClass::NoSipi
            } else {
                TimeoutClass::SipiSentNoArrive
            };
            let mut row = LedgerRow::make(self.book.len() as u64 + 1, s.id, class, now, s);
            row.fp = row.compute_fp();
            self.book.push(row);
            if let Some(x) = self.aps.get_mut(i) {
                x.in_ledger = true;
            }
            added = added.wrapping_add(1);
        }
        Ok(added)
    }

    /// 迟到达补记（超时后才置位的 AP 追加类 C 行·不改旧行）。
    pub fn note_late(&mut self, ap: u8, at: u64) -> Result<u64, V326Code> {
        let i = self.check(ap)?;
        self.tick(at)?;
        let s = match self.aps.get(i) {
            Some(x) => x.clone(),
            None => return Err(V326Code::ApRange),
        };
        if !s.late {
            return Ok(0);
        }
        if self.book.len() >= LEDGER_CAP {
            self.bump(V326Code::LedgerFull);
            return Err(V326Code::LedgerFull);
        }
        let mut row = LedgerRow::make(
            self.book.len() as u64 + 1,
            ap,
            TimeoutClass::LateArrive,
            at,
            &s,
        );
        row.fp = row.compute_fp();
        let seq = row.seq;
        self.book.push(row);
        Ok(seq)
    }

    /// 全员到达（栅栏可放行条件）。
    pub fn all_arrived(&self) -> bool {
        self.aps.iter().all(|s| s.arrived() || s.voided())
    }

    /// 放行（未齐即放行 = EarlyRelease）。
    pub fn release(&mut self) -> Result<(), V326Code> {
        if !self.all_arrived() {
            self.bump(V326Code::EarlyRelease);
            return Err(V326Code::EarlyRelease);
        }
        self.n_release = self.n_release.wrapping_add(1);
        Ok(())
    }

    /// 计数漂移核对（`n_arrive` == 哨兵实况）。
    pub fn count_ok(&self) -> bool {
        let real = self.aps.iter().filter(|s| s.arrived()).count() as u64;
        if real != self.n_arrive {
            return false;
        }
        // 到达 + 作废 == 曾被置位的槽数（无槽凭空消失）。
        real.wrapping_add(self.aps.iter().filter(|s| s.voided()).count() as u64)
            == self.n_arrive.wrapping_add(self.n_void)
    }

    /// 守恒总门。
    pub fn conserved(&self) -> bool {
        self.count_ok()
            && self.n_dup + self.n_forge + self.n_arrive >= self.n_arrive
            && self.book.len() <= LEDGER_CAP
    }

    /// 分类幂等核对（每行复算 == 原分类· 且指纹完好）。
    pub fn class_idempotent(&self) -> bool {
        let mut prev = 0u64;
        for r in self.book.iter() {
            if r.reclassify() != r.class || !r.fp_intact() || r.seq <= prev {
                return false;
            }
            prev = r.seq;
        }
        true
    }

    /// 分类分布（三类计数）。
    pub fn class_counts(&self) -> (u32, u32, u32) {
        let mut a = 0u32;
        let mut b = 0u32;
        let mut c = 0u32;
        for r in self.book.iter() {
            match r.class {
                TimeoutClass::NoSipi => a = a.wrapping_add(1),
                TimeoutClass::SipiSentNoArrive => b = b.wrapping_add(1),
                TimeoutClass::LateArrive => c = c.wrapping_add(1),
                TimeoutClass::None => {}
            }
        }
        (a, b, c)
    }

    /// 渲染超时账 + 实时槽况（排障真源）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("ap  sentinel    set_by at          sipi_atrenews late ledger\n");
        for a in self.aps.iter() {
            s.push_str(&format!(
                "{:<3} 0x{:08X}  {:<6} {:<13} {:<8}{:<6} {:<5} {}\n",
                a.id,
                a.sentinel,
                a.set_by,
                a.set_at,
                a.sipi_at,
                a.renews,
                a.late,
                a.in_ledger
            ));
        }
        s.push_str("seq ap  class           at          sipisent arrive renews fp\n");
        for r in self.book.iter() {
            s.push_str(&format!(
                "{:<3} {:<3} {:<14} {:<11} {:<8} {:<6} {:<6} 0x{:016X}\n",
                r.seq,
                r.ap,
                r.class.cn(),
                r.at,
                r.at_sipisent,
                r.at_arrive,
                r.at_renews,
                r.fp
            ));
        }
        let (a, b, c) = self.class_counts();
        s.push_str(&format!(
            "classes NoSipi={} SipiSentNoArrive={} LateArrive={} budget={} now={} start={} renews={}\n",
            a,
            b,
            c,
            self.budget(),
            self.now,
            self.fence_start(),
            self.renew_budget
        ));
        s
    }
}

// --------------------------------------------------------------------------
// 判据
// --------------------------------------------------------------------------

/// UNX-F0326 判据全集（12 条）。
pub fn run_unx326_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx326");

    // ---- C326-01 诊断段闭集与建议完备 ----
    let seg_ok = V326Code::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.val() == V326_SEG_BASE + i as u16);
    let remedy_ok = V326Code::ALL.iter().all(|c| c.remedy().len() >= 10);
    cs.add("C326-01 诊断段闭集", seg_ok && remedy_ok, "12 码连续+建议完备");

    // ---- C326-02 哨兵三态魔数闭集 ----
    let magic_ok = SENT_UNSET == 0
        && SENT_ARRIVED == 0xA5A5_5A5A
        && SENT_VOID == 0xDEAD_BEEF
        && SENT_ARRIVED != SENT_VOID;
    let a0 = ApSlot::new(0);
    let state_ok = a0.pending() && !a0.arrived() && !a0.voided() && a0.sentinel == SENT_UNSET;
    cs.add("C326-02 哨兵三态", magic_ok && state_ok, "未达/到达/作废三态闭集");

    // ---- C326-03 原子置位：恰好一次生效 ----
    let mut s3 = Sentinel::new(4);
    let _ = s3.open_fence(1_000);
    let t0 = 1_500;
    let _ = s3.send_sipi(1, t0);
    let first = s3.arrive(1, 1, t0 + 10);
    let second = s3.arrive(1, 1, t0 + 20);
    let at_ok = first == Ok(t0 + 10)
        && second == Err(V326Code::AlreadySet)
        && s3.n_arrive == 1
        && s3.n_dup == 1
        && s3.aps.get(1).map(|x| x.sentinel) == Some(SENT_ARRIVED)
        && s3.aps.get(1).map(|x| x.set_by) == Some(1)
        && s3.aps.get(1).map(|x| x.set_at) == Some(t0 + 10);
    cs.add("C326-03 原子置位", at_ok, "第二次被拒+计数只加一次");

    // ---- C326-04 伪造就位拦截（who≠ap） ----
    let mut s4 = Sentinel::new(4);
    let _ = s4.open_fence(1_000);
    let forged1 = s4.arrive(2, 0, 1_100); // who=0（核心代置）
    let forged2 = s4.arrive(2, 3, 1_110); // who=3（张冠李戴）
    let forge_ok = forged1 == Err(V326Code::SentinelForged)
        && forged2 == Err(V326Code::SentinelForged)
        && s4.n_forge == 2
        && s4.n_arrive == 0
        && s4.aps.get(2).map(|x| x.sentinel) == Some(SENT_UNSET)
        && s4.stat(V326Code::SentinelForged) == 2;
    cs.add("C326-04 伪造就位拒", forge_ok, "核心代置/张冠李戴全拒零污染");

    // ---- C326-05 作废与作废后到达 ----
    let mut s5 = Sentinel::new(4);
    let _ = s5.open_fence(1_000);
    let v = s5.void_ap(3, 1_200);
    let after = s5.arrive(3, 3, 1_300);
    let void_ok = v.is_ok()
        && after == Err(V326Code::AlreadySet)
        && s5.aps.get(3).map(|x| x.voided()) == Some(true)
        && s5.n_void == 1
        && s5.count_ok();
    // 已到达后作废亦拒（终态不可逆）。
    let mut s5b = Sentinel::new(2);
    let _ = s5b.open_fence(1_000);
    let _ = s5b.arrive(0, 0, 1_050);
    let rev = s5b.void_ap(0, 1_060);
    cs.add(
        "C326-05 作废终态",
        void_ok && rev == Err(V326Code::AlreadySet),
        "作废后到达拒+终态不可逆",
    );

    // ---- C326-06 200ms/AP 预算口径 ----
    let mut s6 = Sentinel::new(4);
    let _ = s6.open_fence(1_000);
    let _ = s6.send_sipi(0, 1_000);
    let b1 = s6.budget();
    let _ = s6.arrive(0, 0, 1_100);
    let b2 = s6.budget();
    let _ = s6.arrive(1, 1, 1_100);
    let b3 = s6.budget();
    let budget_ok = b1 == AP_BUDGET_TICKS
        && b2 == AP_BUDGET_TICKS
        && b3 == AP_BUDGET_TICKS.wrapping_mul(2);
    // 耗尽判定：start+200ms 恰好耗尽。
    let mut s6b = Sentinel::new(2);
    let _ = s6b.open_fence(1_000);
    let _ = s6b.send_sipi(0, 1_000);
    let _ = s6b.tick(1_000 + AP_BUDGET_TICKS - 1);
    let before = s6b.budget_exhausted();
    let _ = s6b.tick(1_000 + AP_BUDGET_TICKS);
    let after2 = s6b.budget_exhausted();
    cs.add(
        "C326-06 预算口径",
        budget_ok && !before && after2,
        "200ms×到达数+边界恰好耗尽",
    );

    // ---- C326-07 超时三类分类齐 ----
    let mut s7 = Sentinel::new(4);
    let _ = s7.open_fence(1_000);
    let _ = s7.send_sipi(0, 1_000); // AP0 已发 → 超时为类 B
    let _ = s7.send_sipi(1, 1_000); // AP1 已发 → 超时为类 B
    // AP2/AP3 未发 SIPI → 超时为类 A
    let _ = s7.tick(1_000 + AP_BUDGET_TICKS + 5_000);
    let added = s7.sweep();
    let (na, sb, _lc) = s7.class_counts();
    let class_ok = added == Ok(4) && na == 2 && sb == 2 && s7.class_idempotent();
    // 三 AP 全到 → 不入账。
    let mut s7b = Sentinel::new(3);
    let _ = s7b.open_fence(1_000);
    for i in 0..3u8 {
        let _ = s7b.send_sipi(i, 1_000);
        let _ = s7b.arrive(i, i, 1_050);
    }
    let _ = s7b.tick(1_000 + AP_BUDGET_TICKS * 10);
    let none = s7b.sweep();
    cs.add(
        "C326-07 三类分类",
        class_ok && none == Ok(0) && s7b.book.is_empty(),
        "类A2+类B2+齐员零入账",
    );

    // ---- C326-08 迟到达补记不改旧行 ----
    let mut s8 = Sentinel::new(2);
    let _ = s8.open_fence(1_000);
    let _ = s8.send_sipi(0, 1_000);
    let _ = s8.tick(1_000 + AP_BUDGET_TICKS + 1_000);
    let _ = s8.sweep();
    let fp_before = s8.book.first().map(|r| r.fp).unwrap_or(0);
    let cls_before = s8.book.first().map(|r| r.class);
    let _ = s8.arrive(0, 0, 1_000 + AP_BUDGET_TICKS + 2_000);
    let late_flag = s8.aps.get(0).map(|x| x.late) == Some(true);
    let seq_base = s8.book.len() as u64;
    let note = s8.note_late(0, 1_000 + AP_BUDGET_TICKS + 2_100);
    let fp_after = s8.book.first().map(|r| r.fp).unwrap_or(0);
    let cls_after = s8.book.first().map(|r| r.class);
    let (_a, _b, c) = s8.class_counts();
    let late_ok = late_flag
        && note == Ok(seq_base + 1)
        && fp_before == fp_after
        && cls_before == cls_after
        && c == 1
        && s8.class_idempotent();
    cs.add(
        "C326-08 迟到达补记",
        late_ok,
        "追加类C行+旧行指纹与分类不变",
    );

    // ---- C326-09 续期上限与空转拒 ----
    let mut s9 = Sentinel::new(2);
    let _ = s9.open_fence(1_000);
    let _ = s9.send_sipi(0, 1_000);
    let mut renew_ok = true;
    for k in 0..MAX_RENEW {
        if s9.renew() != Ok(k + 1) {
            renew_ok = false;
        }
    }
    let over = s9.renew();
    let empty = {
        let mut e = Sentinel::new(1);
        let _ = e.open_fence(1_000);
        let _ = e.arrive(0, 0, 1_050);
        e.renew()
    };
    let renew_guard = renew_ok
        && over == Err(V326Code::RenewExhausted)
        && empty == Err(V326Code::RenewNoPending)
        && s9.stat(V326Code::RenewExhausted) == 1
        && s9.aps.get(0).map(|x| x.renews) == Some(MAX_RENEW);
    cs.add("C326-09 续期纪律", renew_guard, "上限强拒+无在途拒空转");

    // ---- C326-10 提前放行拦截 ----
    let mut s10 = Sentinel::new(3);
    let _ = s10.open_fence(1_000);
    let early = s10.release();
    let _ = s10.arrive(0, 0, 1_050);
    let early2 = s10.release();
    let _ = s10.void_ap(2, 1_060);
    let still = s10.release();
    let _ = s10.arrive(1, 1, 1_070);
    let ok = s10.release();
    let rel_ok = early == Err(V326Code::EarlyRelease)
        && early2 == Err(V326Code::EarlyRelease)
        && still == Err(V326Code::EarlyRelease)
        && ok.is_ok()
        && s10.n_release == 1
        && s10.stat(V326Code::EarlyRelease) == 3;
    cs.add("C326-10 提前放行拒", rel_ok, "未齐三次拒+齐员(含作废)放行");

    // ---- C326-11 栅栏未开/时钟回退/AP 越界 三闸 ----
    let mut s11 = Sentinel::new(2);
    let not_open = s11.sweep();
    let clock = s11.tick(500);
    let back = s11.tick(100);
    let range = s11.arrive(9, 9, 600);
    let gate_ok = not_open == Err(V326Code::NotOpen)
        && clock == Ok(500)
        && back == Err(V326Code::ClockBack)
        && range == Err(V326Code::ApRange)
        && s11.now == 500
        && s11.stat(V326Code::NotOpen) == 1
        && s11.stat(V326Code::ClockBack) == 1
        && s11.stat(V326Code::ApRange) == 1;
    cs.add("C326-11 三闸", gate_ok, "未开拒+回退拒+越界拒");

    // ---- C326-12 分类幂等 + 计数守恒 + 渲染完备 ----
    let (a12, b12, c12) = s7.class_counts();
    let total12 = a12 + b12 + c12;
    let conserve = s7.count_ok()
        && s7.conserved()
        && s7.class_idempotent()
        && total12 == s7.book.len() as u32;
    let txt = s7.render();
    let render_ok = txt.contains("sentinel") && txt.contains("classes") && txt.contains("budget=");
    // 篡改历史行 → 幂等核对必须失败（反证幂等检查非空转）。
    let mut s12 = s7.clone();
    if let Some(r) = s12.book.first_mut() {
        r.class = TimeoutClass::NoSipi;
    }
    let tamper_caught = !s12.class_idempotent();
    cs.add(
        "C326-12 幂等与守恒",
        conserve && render_ok && tamper_caught,
        "三类合计守恒+篡改必检出+看板齐",
    );

    cs
}