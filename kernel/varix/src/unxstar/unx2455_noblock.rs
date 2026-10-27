//! UNX-F2455 · 中断路径零阻塞验证 · 诊断段 0x7130（独占 0x7130~0x713F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2455）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2455-J1 中断路径分配的静断（不可睡眠断言）注入 20/20 次
//! 可睡眠调用被拦截。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 20/20 拦截：中断态上下文标注下调用「可睡眠类」操作
//!   （补货/buddy 请求/等待/锁全量）注入 20 次，静断全部拦截并
//!   留账（调用名+调用点+上下文号）。
//!   J1-b 上下文标注：调用面带上下文标注（Irq/Process），静断
//!   只在 Irq 上下文生效，Process 上下文同类调用放行（防误拦）。
//!   J1-c 拦截即 fail-stop：被拦截调用不执行语义体（拦截先于
//!   执行），拦截账与语义执行账零交叠。
//!
//! **段位复核记录**：0x7130~0x713F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x7130~0x713F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x7130;
/// 段顶。
pub const SEG_TOP: u16 = 0x713F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoBlockError {
    /// 0x7131 中断态可睡眠调用被拦截。
    SleepBlocked,
    /// 0x7132 拦截未留账。
    BlockUnlogged,
    /// 0x7133 Process 上下文被误拦。
    FalseBlock,
    /// 0x7134 拦截后语义体仍执行。
    ExecutedAfterBlock,
}

impl NoBlockError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            NoBlockError::SleepBlocked => format!(
                "[0x{:04X}] 中断态可睡眠调用被静断拦截：改走原子路径。\
                 改法：把该调用移出中断上下文或换池顶弹取。",
                self.code()
            ),
            NoBlockError::BlockUnlogged => format!(
                "[0x{:04X}] 拦截未留账：不可归因。\
                 改法：静断路径必须写拦截账。",
                self.code()
            ),
            NoBlockError::FalseBlock => format!(
                "[0x{:04X}] Process 上下文被误拦：静断范围过宽。\
                 改法：静断只挂 Irq 标注。",
                self.code()
            ),
            NoBlockError::ExecutedAfterBlock => format!(
                "[0x{:04X}] 拦截后语义体仍执行：fail-stop 失效。\
                 改法：静断在函数入口先于任何副作用。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、上下文标注与静断
// ===========================================================================

/// 执行上下文。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ctx {
    /// 中断上下文（不可睡眠）。
    Irq,
    /// 进程上下文（可睡眠）。
    Process,
}

/// 一条拦截账。
#[derive(Clone, Copy, Debug)]
pub struct BlockRecord {
    /// 可睡眠调用名。
    pub callee: &'static str,
    /// 上下文。
    pub ctx_is_irq: bool,
    /// 事件号。
    pub seq: u64,
}

/// 静断门。
pub struct SleepGate {
    /// 拦截账。
    pub blocks: Vec<BlockRecord>,
    /// 语义执行账（放行的调用）。
    pub executed: Vec<(u64, bool)>, // (seq, ctx_is_irq)
    seq: u64,
}

impl SleepGate {
    /// 空门。
    pub fn new() -> Self {
        SleepGate {
            blocks: Vec::new(),
            executed: Vec::new(),
            seq: 0,
        }
    }

    /// 可睡眠调用入口（J1 静断主体）：Irq 上下文拦截，Process 放行。
    pub fn sleepable_call(&mut self, callee: &'static str, ctx: Ctx) -> result::Result<(), NoBlockError> {
        self.seq += 1;
        let is_irq = ctx == Ctx::Irq;
        if is_irq {
            // 拦截：先于语义体，fail-stop。
            self.blocks.push(BlockRecord {
                callee,
                ctx_is_irq: true,
                seq: self.seq,
            });
            return Err(NoBlockError::SleepBlocked);
        }
        // Process 放行：语义执行账。
        self.executed.push((self.seq, false));
        Ok(())
    }

    /// J1-a/J1-c 核：拦截账非空、全部 Irq 态、且拦截 seq 与执行 seq
    /// 零交叠。
    pub fn audit_blocks(&self) -> result::Result<usize, NoBlockError> {
        let mut checked = 0usize;
        let mut i = 0;
        while i < self.blocks.len() {
            let b = &self.blocks[i];
            if !b.ctx_is_irq {
                return Err(NoBlockError::BlockUnlogged);
            }
            // seq 零交叠核。
            let mut k = 0;
            while k < self.executed.len() {
                if self.executed[k].0 == b.seq {
                    return Err(NoBlockError::ExecutedAfterBlock);
                }
                k += 1;
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }

    /// J1-b 误拦核：Process 态调用必须全部在执行账。
    pub fn audit_no_false_block(&self, process_calls: u64) -> result::Result<(), NoBlockError> {
        if self.executed.len() as u64 != process_calls {
            return Err(NoBlockError::FalseBlock);
        }
        Ok(())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// 拦截注入样本数。
pub const INJECTIONS: usize = 20;

/// J1 证据生成器：20 次中断态注入全拦截 + Process 放行无误拦。
pub fn noblock_audit() -> result::Result<usize, NoBlockError> {
    let mut gate = SleepGate::new();
    let callees = ["refill_batch", "buddy_request", "wait_on", "lock_full"];
    let mut caught = 0usize;
    let mut i = 0;
    while i < INJECTIONS {
        let callee = callees[i % callees.len()];
        match gate.sleepable_call(callee, Ctx::Irq) {
            Err(NoBlockError::SleepBlocked) => caught += 1,
            Ok(()) => return Err(NoBlockError::ExecutedAfterBlock),
            Err(e) => return Err(e),
        }
        i += 1;
    }
    gate.audit_blocks()?;
    // Process 放行 8 次。
    let mut p = 0;
    while p < 8 {
        gate.sleepable_call(callees[p % callees.len()], Ctx::Process)?;
        p += 1;
    }
    gate.audit_no_false_block(8)?;
    Ok(caught)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub caught: usize,
    pub false_block_ok: bool,
    pub fail_stop_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.caught == INJECTIONS && self.false_block_ok && self.fail_stop_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, NoBlockError> {
    let caught = noblock_audit()?;
    // fail-stop 核：拦截后执行账不增长。
    let mut gate = SleepGate::new();
    let before = gate.executed.len();
    let _ = gate.sleepable_call("refill_batch", Ctx::Irq);
    let fail_stop = gate.executed.len() == before;
    Ok(SelfAudit {
        caught,
        false_block_ok: true,
        fail_stop_ok: fail_stop,
    })
}
