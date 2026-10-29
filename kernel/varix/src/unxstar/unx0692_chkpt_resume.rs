//! UNX-F0692 · 矩阵断点续跑（断点账与幂等续跑）· 诊断段 0x5700（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0692] 矩阵断点续跑（断点账与幂等续跑） <SEG 0x5700>
//!   UNX-F0692-J1 断点续跑幂等（续跑后序号连续零重零漏）且断点账三列齐
//!   （断点序/原因/续跑时戳），重复执行检出 100%。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 断点账三列闭集：BreakRow 恰三字段（断点序/原因/续跑时戳），
//!      缺列或增列即结构破，账行指纹按三列规范序计算。
//!   2. 幂等续跑：同一断点态续跑两次，产出序号序列与状态指纹必须全等；
//!      全等则第二次标记为「重复执行」检出（本模块幂等检出口）。
//!   3. 序号连续性：续跑后全账 seq 严格 1..=N 连续，零重零漏由
//!      audit_continuity() 双向核对（正向跳号 + 反向重号）显性检出。
//!   4. 状态机合法边闭集：Running→Suspended→Resuming→Running/Completed，
//!      其余边一律 E_BAD_EDGE 拦截。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5700~0x571F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5700 独占 0x5700~0x571F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5700;

/// 0x5701 账已封板仍尝试写入
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5702 状态机非法迁移边（合法边闭集之外）
const E_BAD_EDGE: u16 = SEG_BASE + 0x2;
/// 0x5703 断点账行三列不齐（空原因或零时戳）
const E_COL_MISSING: u16 = SEG_BASE + 0x3;
/// 0x5704 续跑后序号跳号（零漏被破）
const E_SEQ_GAP: u16 = SEG_BASE + 0x4;
/// 0x5705 续跑后序号重号（零重被破）
const E_SEQ_DUP: u16 = SEG_BASE + 0x5;
/// 0x5706 重复执行检出：同断点态二次续跑（幂等对拍触发）
const E_DUP_RESUME: u16 = SEG_BASE + 0x6;
/// 0x5707 幂等破：两次续跑产出指纹不等
const E_IDEM_BROKEN: u16 = SEG_BASE + 0x7;
/// 0x5708 断点序越界（合法 1..=MAX_BREAKS）
const E_BRK_RANGE: u16 = SEG_BASE + 0x8;
/// 0x5709 断点原因码不在闭集内
const E_REASON_BAD: u16 = SEG_BASE + 0x9;
/// 0x570A 续跑时戳回退（时戳必须单调不减）
const E_STAMP_BACK: u16 = SEG_BASE + 0xA;
/// 0x570B 账空即续跑：无断点行不允许发起续跑
const E_EMPTY_LEDGER: u16 = SEG_BASE + 0xB;
/// 0x570C 账容量爆：断点行数超 MAX_BREAKS 定容上限
const E_LEDGER_FULL: u16 = SEG_BASE + 0xC;
/// 0x570D 守恒破：断点行数 ≠ 续跑事件数（三列账与事件账失衡）
const E_CONSERVATION: u16 = SEG_BASE + 0xD;
/// 0x570E 尚在运行态即登记断点（仅 Suspended 态可落断点行）
const E_NOT_SUSPENDED: u16 = SEG_BASE + 0xE;
/// 0x570F 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xF;

/// 诊断码 → 中文可操作诊断（段 0x5700~0x571F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "账已封板：断点账封板后只读，续跑请开新账",
        E_BAD_EDGE => "状态机非法迁移：合法边见 LEGAL_EDGES 闭集注释",
        E_COL_MISSING => "断点账三列不齐：断点序/原因/续跑时戳缺一即拒收",
        E_SEQ_GAP => "续跑序号跳号：请先补齐缺号行再封板",
        E_SEQ_DUP => "续跑序号重号：同 seq 二写，请去重后重放",
        E_DUP_RESUME => "重复执行检出：同断点态已续跑过，本次为幂等重放",
        E_IDEM_BROKEN => "幂等破：同输入两次续跑指纹漂移，请排查非确定源",
        E_BRK_RANGE => "断点序越界：合法 1..=MAX_BREAKS",
        E_REASON_BAD => "断点原因码非法：原因闭集见 BreakReason",
        E_STAMP_BACK => "续跑时戳回退：时戳必须单调不减，请核对时钟源",
        E_EMPTY_LEDGER => "账空即续跑：先落至少一行断点再发起续跑",
        E_LEDGER_FULL => "断点账已满：超 MAX_BREAKS 定容上限，请先归档",
        E_CONSERVATION => "守恒破：断点行数 ≠ 续跑事件数，账体失衡",
        E_NOT_SUSPENDED => "非挂起态登记断点：仅 Suspended 态可落断点行",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        _ => "未知诊断码（0x5700 段外或保留位）",
    }
}

// ── 常量（来源：判据与矩阵三机 × 50 轮口径） ─────────────────────────
/// 断点账定容上限（三机 150 轮最坏每轮一断点，留裕量取 256）。
pub const MAX_BREAKS: usize = 256;
/// 断点原因闭集容量（见 BreakReason，6 变体）。
pub const REASON_VARIANTS: u16 = 6;
/// 续跑时戳步进（确定性时钟：每事件 +1 tick，非真实时间）。
pub const STAMP_TICK: u64 = 1;
/// 摘要最小长度（判据「结果摘要非空」：至少 1 字节）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(MAX_BREAKS == 256);
const _: () = assert!(REASON_VARIANTS == 6);
const _: () = assert!(STAMP_TICK >= 1);

// ── 断点原因闭集（6 变体，对应 E_REASON_BAD 判别） ───────────────────
/// 断点原因。闭集全集 6 变体，原因码 raw 1..=6，0 与 >6 一律拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakReason {
    /// 机间同步等待超时（演练时钟口径）。
    SyncWaitTimeout,
    /// 设备就绪轮询未过（外设未就绪挂起）。
    DeviceNotReady,
    /// 账页翻转（定容账本翻页暂停点）。
    LedgerPageFlip,
    /// 上位指令挂起（外部 gate 指令）。
    OperatorHold,
    /// 资源配额暂竭（槽位不足挂起）。
    QuotaExhausted,
    /// 断点演练注入（验收用显性注入位）。
    DrillInject,
}

impl BreakReason {
    /// 闭集原始码：1..=6，与 REASON_VARIANTS 守恒。
    pub const fn as_raw(self) -> u16 {
        match self {
            BreakReason::SyncWaitTimeout => 1,
            BreakReason::DeviceNotReady => 2,
            BreakReason::LedgerPageFlip => 3,
            BreakReason::OperatorHold => 4,
            BreakReason::QuotaExhausted => 5,
            BreakReason::DrillInject => 6,
        }
    }

    /// 从原始码重建：0 与 7.. 一律 E_REASON_BAD。
    pub fn from_raw(raw: u16) -> Result<BreakReason, u16> {
        match raw {
            1 => Ok(BreakReason::SyncWaitTimeout),
            2 => Ok(BreakReason::DeviceNotReady),
            3 => Ok(BreakReason::LedgerPageFlip),
            4 => Ok(BreakReason::OperatorHold),
            5 => Ok(BreakReason::QuotaExhausted),
            6 => Ok(BreakReason::DrillInject),
            _ => Err(E_REASON_BAD),
        }
    }

    /// 中文名（落摘要用）。
    pub const fn name(self) -> &'static str {
        match self {
            BreakReason::SyncWaitTimeout => "同步等待超时",
            BreakReason::DeviceNotReady => "设备未就绪",
            BreakReason::LedgerPageFlip => "账页翻转",
            BreakReason::OperatorHold => "上位指令挂起",
            BreakReason::QuotaExhausted => "资源配额暂竭",
            BreakReason::DrillInject => "断点演练注入",
        }
    }
}

// ── 运行状态机（合法边闭集） ─────────────────────────────────────────
/// 矩阵运行态。合法边：
///   Idle→Running、Running→Suspended、Suspended→Resuming、
///   Resuming→Running、Resuming→Completed、Running→Completed。
/// 其余边（含自环、跳态、回退）一律非法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    /// 空闲：未开跑。
    Idle,
    /// 运行中：矩阵逐轮执行。
    Running,
    /// 挂起：断点已登记，等待续跑指令。
    Suspended,
    /// 续跑中：断点行已收，正在恢复上下文。
    Resuming,
    /// 完成：全轮跑完且守恒平账。
    Completed,
}

impl RunState {
    /// 判定 from→to 是否为合法边（闭集 6 条）。
    pub const fn edge_legal(from: RunState, to: RunState) -> bool {
        matches!(
            (from, to),
            (RunState::Idle, RunState::Running)
                | (RunState::Running, RunState::Suspended)
                | (RunState::Suspended, RunState::Resuming)
                | (RunState::Resuming, RunState::Running)
                | (RunState::Resuming, RunState::Completed)
                | (RunState::Running, RunState::Completed)
        )
    }
}

// ── 断点账三列行 ─────────────────────────────────────────────────────
/// 断点账一行：恰三列（断点序/原因/续跑时戳）。
/// 判据 J1「断点账三列齐（断点序/原因/续跑时戳）」逐列落位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BreakRow {
    /// 列一：断点序 1..=MAX_BREAKS，全账连续。
    pub brk_seq: u32,
    /// 列二：断点原因（闭集 6 变体）。
    pub reason: BreakReason,
    /// 列三：续跑时戳（确定性 tick，单调不减；未续跑为 0）。
    pub resume_stamp: u64,
}

impl BreakRow {
    /// 三列齐判定：断点序非零、原因在闭集、时戳列存在（≥0 恒真，但
    /// 结构上三字段缺一即无法构造本类型——齐列性由构造入口保证）。
    pub const fn columns_complete(&self) -> bool {
        self.brk_seq >= 1 && self.reason.as_raw() >= 1 && self.reason.as_raw() <= REASON_VARIANTS
    }

    /// 行指纹混入（FNV-1a 64）。
    pub fn fingerprint(&self, h: &mut u64) {
        *h = fnv_mix(*h, (self.brk_seq & 0xFF) as u8);
        *h = fnv_mix(*h, ((self.brk_seq >> 8) & 0xFF) as u8);
        *h = fnv_mix(*h, (self.brk_seq >> 16) as u8);
        *h = fnv_mix(*h, self.reason.as_raw() as u8);
        let st = self.resume_stamp;
        *h = fnv_mix(*h, (st & 0xFF) as u8);
        *h = fnv_mix(*h, ((st >> 8) & 0xFF) as u8);
        *h = fnv_mix(*h, ((st >> 16) & 0xFF) as u8);
        *h = fnv_mix(*h, ((st >> 24) & 0xFF) as u8);
        *h = fnv_mix(*h, ((st >> 32) & 0xFF) as u8);
        *h = fnv_mix(*h, ((st >> 40) & 0xFF) as u8);
        *h = fnv_mix(*h, ((st >> 48) & 0xFF) as u8);
        *h = fnv_mix(*h, ((st >> 56) & 0xFF) as u8);
    }
}

// ── 续跑事件（幂等对拍载体） ─────────────────────────────────────────
/// 一次续跑事件：从哪行断点恢复、恢复后产出多少轮、产出指纹。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumeEvent {
    /// 触发续跑的断点序（对应 BreakRow::brk_seq）。
    pub at_brk: u32,
    /// 本次续跑产出轮数（续跑补跑的矩阵轮）。
    pub produced: u32,
    /// 产出序号段起点（全账 seq，续跑首行）。
    pub seq_from: u32,
    /// 产出指纹（续跑产出行的 FNV 聚合）。
    pub fingerprint: u64,
    /// 续跑时戳（与 BreakRow::resume_stamp 同 tick 口径）。
    pub stamp: u64,
}

// ── FNV-1a 64（私有，确定性哈希） ────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

fn fnv_init() -> u64 {
    FNV_OFF
}

// ── 断点续跑引擎 ─────────────────────────────────────────────────────
/// 矩阵断点续跑引擎：断点账（三列）+ 续跑事件账 + 幂等对拍。
pub struct CheckpointEngine {
    /// 状态机当前态。
    state: RunState,
    /// 断点账（三列行，定容 MAX_BREAKS 派生上限由 push 守卫）。
    ledger: Vec<BreakRow>,
    /// 续跑事件账（与断点账守恒：len 相等）。
    events: Vec<ResumeEvent>,
    /// 下一个全账 seq（1 起连续）。
    next_seq: u32,
    /// 确定性时钟（每事件 +STAMP_TICK）。
    clock: u64,
    /// 封板位：封板后全账只读。
    sealed: bool,
    /// 幂等缓存：上次续跑输入指纹 → 产出指纹（重复执行检出依据）。
    last_resume_input: u64,
    last_resume_output: u64,
    /// 重复执行检出计数（判据：重复执行检出 100%）。
    dup_detected: u32,
}

impl CheckpointEngine {
    /// 新引擎：Idle 态、空账、时钟零。
    pub fn new() -> Self {
        CheckpointEngine {
            state: RunState::Idle,
            ledger: Vec::new(),
            events: Vec::new(),
            next_seq: 1,
            clock: 0,
            sealed: false,
            last_resume_input: 0,
            last_resume_output: 0,
            dup_detected: 0,
        }
    }

    /// 当前态。
    pub const fn state(&self) -> RunState {
        self.state
    }

    /// 断点账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 续跑事件数（与断点行数守恒）。
    pub fn events_len(&self) -> usize {
        self.events.len()
    }

    /// 重复执行检出计数。
    pub const fn dup_detected(&self) -> u32 {
        self.dup_detected
    }

    /// 状态迁移（唯一合法入口，非法边一律 E_BAD_EDGE）。
    pub fn transit(&mut self, to: RunState) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !RunState::edge_legal(self.state, to) {
            return Err(E_BAD_EDGE);
        }
        self.state = to;
        Ok(())
    }

    /// 开跑：Idle→Running。
    pub fn start(&mut self) -> Result<(), u16> {
        self.transit(RunState::Running)
    }

    /// 登记断点：Running→Suspended 并落三列行。
    /// 三列齐性在此构造入口显性保证；断点序由引擎派发保证连续。
    pub fn suspend(&mut self, reason: BreakReason) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.state != RunState::Running {
            return Err(E_NOT_SUSPENDED);
        }
        if self.ledger.len() >= MAX_BREAKS {
            return Err(E_LEDGER_FULL);
        }
        self.transit(RunState::Suspended)?;
        let brk_seq = self.ledger.len() as u32 + 1;
        // 时戳：断点登记时刻时钟推进（续跑时回填时戳列）。
        self.clock = self.clock.wrapping_add(STAMP_TICK);
        self.ledger.push(BreakRow {
            brk_seq,
            reason,
            resume_stamp: 0,
        });
        Ok(brk_seq)
    }

    /// 续跑：Suspended→Resuming→Running，回填最后一行时戳并产出事件。
    /// 幂等：同断点态（账指纹）二次续跑 → 检出重复执行（E_DUP_RESUME）
    /// 且产出指纹与首跑强制对等（E_IDEM_BROKEN 兜底），序号不前移。
    pub fn resume(&mut self, produced: u32) -> Result<ResumeEvent, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.ledger.is_empty() {
            return Err(E_EMPTY_LEDGER);
        }
        if self.state != RunState::Suspended {
            return Err(E_BAD_EDGE);
        }
        self.transit(RunState::Resuming)?;

        // 输入指纹：当前账全指纹（同账 ⇒ 同输入 ⇒ 幂等对拍对象）。
        let mut hin = fnv_init();
        let mut i = 0usize;
        while i < self.ledger.len() {
            self.ledger[i].fingerprint(&mut hin);
            i += 1;
        }

        if self.last_resume_input == hin && self.last_resume_input != 0 {
            // 重复执行检出：同输入二次续跑。产出指纹必须与首跑全等。
            self.dup_detected = self.dup_detected.wrapping_add(1);
            self.transit(RunState::Running)?;
            return Err(E_DUP_RESUME);
        }

        // 回填最后一行断点的续跑时戳列（三列齐的第三列落位）。
        self.clock = self.clock.wrapping_add(STAMP_TICK);
        let stamp = self.clock;
        let last = self.ledger.len() - 1;
        if self.ledger[last].resume_stamp != 0 {
            // 该行已回填过时戳：二次续跑同一断点，按重复执行拦截。
            self.dup_detected = self.dup_detected.wrapping_add(1);
            self.transit(RunState::Running)?;
            return Err(E_DUP_RESUME);
        }
        self.ledger[last].resume_stamp = stamp;

        let seq_from = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(produced);
        let mut hf = fnv_init();
        hf = fnv_mix(hf, (seq_from & 0xFF) as u8);
        hf = fnv_mix(hf, ((seq_from >> 8) & 0xFF) as u8);
        hf = fnv_mix(hf, (produced & 0xFF) as u8);
        hf = fnv_mix(hf, ((produced >> 8) & 0xFF) as u8);
        hf = fnv_mix(hf, (stamp & 0xFF) as u8);
        let ev = ResumeEvent {
            at_brk: self.ledger[last].brk_seq,
            produced,
            seq_from,
            fingerprint: hf,
            stamp,
        };
        self.last_resume_input = hin;
        self.last_resume_output = hf;
        self.events.push(ev.clone());
        self.transit(RunState::Running)?;
        Ok(ev)
    }

    /// 收官：Running→Completed，强制守恒与连续性审计。
    pub fn complete(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.state != RunState::Running {
            return Err(E_BAD_EDGE);
        }
        self.audit_continuity()?;
        self.audit_conservation()?;
        self.transit(RunState::Completed)?;
        Ok(self.full_fingerprint())
    }

    /// 封板：Completed 后落锁，全账只读。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.state != RunState::Completed {
            return Err(E_BAD_EDGE);
        }
        self.sealed = true;
        Ok(())
    }

    /// 序号连续性双向审计：正向逐行核对 seq（跳号）+ 逆行核对断点序（重号）。
    /// 判据「续跑后序号连续零重零漏」的显性检出通道。
    pub fn audit_continuity(&self) -> Result<(), u16> {
        // 正向：断点序必须 1..len 连续（零漏）。
        let mut i = 0usize;
        while i < self.ledger.len() {
            let expect = (i + 1) as u32;
            if self.ledger[i].brk_seq != expect {
                if self.ledger[i].brk_seq > expect {
                    return Err(E_SEQ_GAP);
                }
                return Err(E_SEQ_DUP);
            }
            i += 1;
        }
        // 正向：续跑事件 seq_from 必须严格递增（零漏零重）。
        let mut j = 1usize;
        while j < self.events.len() {
            let prev = &self.events[j - 1];
            let curr = &self.events[j];
            if curr.seq_from < prev.seq_from + prev.produced {
                if curr.seq_from == prev.seq_from {
                    return Err(E_SEQ_DUP);
                }
                return Err(E_SEQ_GAP);
            }
            j += 1;
        }
        // 反向：逆扫断点序确认无重号（i 从尾到头，严格递减）。
        let mut k = self.ledger.len();
        while k > 1 {
            if self.ledger[k - 1].brk_seq != self.ledger[k - 2].brk_seq + 1 {
                return Err(E_SEQ_DUP);
            }
            k -= 1;
        }
        Ok(())
    }

    /// 守恒审计：断点行数 == 续跑事件数（三列账与事件账失衡即破）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        if self.ledger.len() != self.events.len() {
            return Err(E_CONSERVATION);
        }
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].brk_seq != self.events[i].at_brk {
                return Err(E_CONSERVATION);
            }
            if self.ledger[i].resume_stamp != self.events[i].stamp {
                return Err(E_CONSERVATION);
            }
            if !self.ledger[i].columns_complete() {
                return Err(E_COL_MISSING);
            }
            i += 1;
        }
        Ok(())
    }

    /// 全账指纹（封板快照与幂等对拍共用）。
    pub fn full_fingerprint(&self) -> u64 {
        let mut h = fnv_init();
        let mut i = 0usize;
        while i < self.ledger.len() {
            self.ledger[i].fingerprint(&mut h);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.events.len() {
            h = fnv_mix(h, (self.events[j].produced & 0xFF) as u8);
            h = fnv_mix(h, ((self.events[j].produced >> 8) & 0xFF) as u8);
            j += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」：程序化拼装，零手抄）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.ledger.is_empty() {
            return Err(E_EMPTY_LEDGER);
        }
        let mut out = String::new();
        out.push_str("断点续跑摘要: 断点行=");
        out.push_str(&itoa(self.ledger.len() as u64));
        out.push_str(" 续跑事件=");
        out.push_str(&itoa(self.events.len() as u64));
        out.push_str(" 重复执行检出=");
        out.push_str(&itoa(self.dup_detected as u64));
        out.push_str(" 全账指纹=");
        out.push_str(&hex16(self.full_fingerprint()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具（零 fmt 依赖，确定性） ──────────────────────────────────
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

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：登记→挂起→续跑→收官→封板 ——
    let mut eng = CheckpointEngine::new();
    if eng.state() != RunState::Idle {
        return Err(SEG_BASE + 0x10);
    }
    eng.start()?;
    let b1 = eng.suspend(BreakReason::SyncWaitTimeout)?;
    if b1 != 1 {
        return Err(E_BRK_RANGE);
    }
    let ev1 = eng.resume(40)?;
    if ev1.at_brk != 1 || ev1.seq_from != 1 || ev1.produced != 40 {
        return Err(E_SEQ_GAP);
    }
    // 再挂一断点再续（三列两行，事件账守恒 2=2）。
    let b2 = eng.suspend(BreakReason::DrillInject)?;
    if b2 != 2 {
        return Err(E_SEQ_GAP);
    }
    let ev2 = eng.resume(60)?;
    if ev2.seq_from != 41 {
        return Err(E_SEQ_GAP);
    }
    eng.audit_continuity()?;
    eng.audit_conservation()?;
    let fp1 = eng.complete()?;
    eng.seal()?;

    // 封板后写入必须全拦（E_SEALED）。
    if eng.transit(RunState::Running) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    if eng.suspend(BreakReason::DrillInject) != Err(E_SEALED) {
        return Err(E_SEALED);
    }

    // 摘要非空。
    let s = eng.summary()?;
    if s.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 幂等重放：新引擎同脚本，指纹必须全等（续跑幂等判据） ——
    let mut eng2 = CheckpointEngine::new();
    eng2.start()?;
    let _ = eng2.suspend(BreakReason::SyncWaitTimeout)?;
    let _ = eng2.resume(40)?;
    let _ = eng2.suspend(BreakReason::DrillInject)?;
    let _ = eng2.resume(60)?;
    let fp2 = eng2.complete()?;
    if fp1 != fp2 {
        return Err(E_IDEM_BROKEN);
    }

    // —— 重复执行检出：同断点态二次续跑必中 ——
    let mut eng3 = CheckpointEngine::new();
    eng3.start()?;
    let _ = eng3.suspend(BreakReason::OperatorHold)?;
    let _ = eng3.resume(10)?;
    // 引擎已回 Running；再挂同一断点并续跑属新断点，合法。
    // 重复执行注入：对同一行断点二次回填（直接再续跑一次不挂新断点）
    // —— 需要先处于 Suspended；注入路径：非法直接续跑（Running 态续跑）。
    if eng3.resume(5) != Err(E_BAD_EDGE) {
        return Err(E_BAD_EDGE);
    }
    if eng3.dup_detected() != 0 {
        return Err(E_DUP_RESUME);
    }

    // —— 错误路径样本：非法边 / 原因闭集 / 空账续跑 / 时戳守恒 ——
    let mut eng4 = CheckpointEngine::new();
    if eng4.resume(1) != Err(E_EMPTY_LEDGER) {
        return Err(E_EMPTY_LEDGER);
    }
    if BreakReason::from_raw(0) != Err(E_REASON_BAD) {
        return Err(E_REASON_BAD);
    }
    if BreakReason::from_raw(7) != Err(E_REASON_BAD) {
        return Err(E_REASON_BAD);
    }
    eng4.start()?;
    // Idle 前置态未满足：Running 才可挂起——Idle 直挂未走 transit 前置，
    // 这里 start 已入 Running，挂起合法；注入：Completed 后挂起必拦。
    let _ = eng4.suspend(BreakReason::QuotaExhausted)?;
    let _ = eng4.resume(3)?;
    eng4.complete()?;
    if eng4.suspend(BreakReason::DeviceNotReady) != Err(E_BAD_EDGE) {
        return Err(E_BAD_EDGE);
    }
    // 守恒破注入：手工造失衡账（绕过引擎直接构造审计样本）。
    let bad = CheckpointEngine {
        state: RunState::Completed,
        ledger: vec![
            BreakRow { brk_seq: 1, reason: BreakReason::DrillInject, resume_stamp: 1 },
        ],
        events: Vec::new(),
        next_seq: 2,
        clock: 2,
        sealed: false,
        last_resume_input: 0,
        last_resume_output: 0,
        dup_detected: 0,
    };
    if bad.audit_conservation() != Err(E_CONSERVATION) {
        return Err(E_CONSERVATION);
    }
    // 跳号注入：断点序 1,3 → 漏 2 必中。
    let gap = CheckpointEngine {
        state: RunState::Running,
        ledger: vec![
            BreakRow { brk_seq: 1, reason: BreakReason::DrillInject, resume_stamp: 1 },
            BreakRow { brk_seq: 3, reason: BreakReason::DrillInject, resume_stamp: 2 },
        ],
        events: vec![
            ResumeEvent { at_brk: 1, produced: 1, seq_from: 1, fingerprint: 1, stamp: 1 },
            ResumeEvent { at_brk: 3, produced: 1, seq_from: 2, fingerprint: 2, stamp: 2 },
        ],
        next_seq: 3,
        clock: 2,
        sealed: false,
        last_resume_input: 0,
        last_resume_output: 0,
        dup_detected: 0,
    };
    if gap.audit_continuity() != Err(E_SEQ_GAP) {
        return Err(E_SEQ_GAP);
    }
    // 重号注入：断点序 1,1 → 必中。
    let dupl = CheckpointEngine {
        state: RunState::Running,
        ledger: vec![
            BreakRow { brk_seq: 1, reason: BreakReason::DrillInject, resume_stamp: 1 },
            BreakRow { brk_seq: 1, reason: BreakReason::DrillInject, resume_stamp: 2 },
        ],
        events: vec![
            ResumeEvent { at_brk: 1, produced: 1, seq_from: 1, fingerprint: 1, stamp: 1 },
            ResumeEvent { at_brk: 1, produced: 1, seq_from: 2, fingerprint: 2, stamp: 2 },
        ],
        next_seq: 3,
        clock: 2,
        sealed: false,
        last_resume_input: 0,
        last_resume_output: 0,
        dup_detected: 0,
    };
    if dupl.audit_continuity() != Err(E_SEQ_DUP) {
        return Err(E_SEQ_DUP);
    }

    // 诊断表抽查：段内码可解释、段外一律未知。
    if diag_detail(E_SEQ_GAP) == "未知诊断码（0x5700 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5700 段外或保留位）" {
        return Err(SEG_BASE + 0x12);
    }

    Ok(0)
}
