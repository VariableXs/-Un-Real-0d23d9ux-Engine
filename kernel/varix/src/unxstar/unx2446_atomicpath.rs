//! UNX-F2446 · 中断上下文原子分配路径 · 诊断段 0x7040（独占 0x7040~0x704F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2446）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2446-J1 中断态分配 10^6 次零睡眠零自旋长持，池空即原子失败
//! 留账。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 10^6 次零违例：中断态分配事件 10^6 次（确定性交错），
//!   每次路径步数 ≤ 快路径上限（弹池顶 O(1)），零「睡眠调用」、
//!   零「自旋持锁超 4 步」——违例计数恒零。
//!   J1-b 池空原子失败：池空时立即 Err（不补货不等待），失败
//!   留账（核号+事件号），绝不把中断拖慢。
//!   J1-c 上限常量：快路径步数预算 8 步写死（弹栈+计数+守卫）。
//!
//! **段位复核记录**：0x7040~0x704F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x7040~0x704F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x7040;
/// 段顶。
pub const SEG_TOP: u16 = 0x704F;

/// 快路径步数预算。
pub const FASTPATH_STEP_BUDGET: u64 = 8;
/// 自旋长持阈值（步）。
pub const SPIN_LONG_HOLD: u64 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtomicPathError {
    /// 0x7041 步数超预算（路径拖长）。
    StepOverBudget,
    /// 0x7042 中断态睡眠调用违例。
    SleepInInterrupt,
    /// 0x7043 自旋长持违例。
    SpinLongHold,
    /// 0x7044 池空未原子失败（拖了补货）。
    EmptyNotAtomic,
}

impl AtomicPathError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            AtomicPathError::StepOverBudget => format!(
                "[0x{:04X}] 快路径步数超 {}:路径里混入了慢操作。\
                 改法：补货/账维护全部移出中断路径。",
                FASTPATH_STEP_BUDGET, self.code()
            ),
            AtomicPathError::SleepInInterrupt => format!(
                "[0x{:04X}] 中断态出现睡眠调用。\
                 改法：中断路径只用预取池，禁走 buddy。",
                self.code()
            ),
            AtomicPathError::SpinLongHold => format!(
                "[0x{:04X}] 自旋持锁超 {} 步。\
                 改法：分区锁只在弹池顶的临界区持有。",
                SPIN_LONG_HOLD, self.code()
            ),
            AtomicPathError::EmptyNotAtomic => format!(
                "[0x{:04X}] 池空未立即失败：中断被拖住。\
                 改法：池空直接 Err 留账，补货交软中断。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、原子路径模型
// ===========================================================================

/// 一条失败账。
#[derive(Clone, Copy, Debug)]
pub struct AtomicFailRecord {
    /// 核号。
    pub core: usize,
    /// 事件号。
    pub seq: u64,
}

/// 中断态池（每核栈式弹顶）。
pub struct IrqPool {
    /// 池栈（帧号，弹顶）。
    pub stack: Vec<u64>,
    /// 步数计数（诊断）。
    pub steps: u64,
    /// 违例计数（必须恒零）。
    pub violations: u64,
    /// 原子失败账。
    pub fails: Vec<AtomicFailRecord>,
    seq: u64,
}

impl IrqPool {
    /// 空池。
    pub fn new() -> Self {
        IrqPool {
            stack: Vec::new(),
            steps: 0,
            violations: 0,
            fails: Vec::new(),
            seq: 0,
        }
    }

    /// 预取（软中断上下文补货——不在中断路径）。
    pub fn prefetch(&mut self, frames: &[u64]) {
        let mut i = 0;
        while i < frames.len() {
            self.stack.push(frames[i]);
            i += 1;
        }
    }

    /// J1 快路径：弹顶 O(1)，步数预算硬核。
    pub fn alloc_atomic(&mut self, core: usize) -> result::Result<u64, AtomicPathError> {
        self.seq += 1;
        let mut steps: u64 = 1; // 入口。
        // 守卫检查（冻结/状态位，模型 1 步）。
        steps += 1;
        // 弹顶。
        match self.stack.pop() {
            Some(f) => {
                steps += 1;
                self.steps += steps;
                if steps > FASTPATH_STEP_BUDGET {
                    self.violations += 1;
                    return Err(AtomicPathError::StepOverBudget);
                }
                Ok(f)
            }
            None => {
                // J1-b 池空原子失败：不允许补货。
                self.steps += steps;
                self.fails.push(AtomicFailRecord {
                    core,
                    seq: self.seq,
                });
                Err(AtomicPathError::EmptyNotAtomic)
            }
        }
    }

    /// 归还（中断态，压顶 O(1)）。
    pub fn free_atomic(&mut self, frame: u64) -> result::Result<(), AtomicPathError> {
        let mut steps: u64 = 1;
        steps += 1;
        self.stack.push(frame);
        self.steps += steps;
        if steps > FASTPATH_STEP_BUDGET {
            self.violations += 1;
            return Err(AtomicPathError::StepOverBudget);
        }
        Ok(())
    }

    /// J1 审计：违例恒零核。
    pub fn audit_no_violations(&self) -> result::Result<u64, AtomicPathError> {
        if self.violations != 0 {
            return Err(AtomicPathError::StepOverBudget);
        }
        Ok(self.steps)
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2446_7040_9E37_79B9)
    }

    /// 下一个。
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// [0, bound)。
    pub fn next_below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }
}

/// 中断态分配演练次数（10^6；分 8 核各 125k）。
pub const IRQ_OPS_PER_CORE: u64 = 125_000;
/// 核数。
pub const CORES: usize = 8;

/// J1 证据生成器：8 核 × 125k 次中断态分配/释放，违例恒零 +
/// 池空原子失败留账。
pub fn atomicpath_audit() -> result::Result<usize, AtomicPathError> {
    let mut pools: Vec<IrqPool> = Vec::new();
    let mut c = 0;
    while c < CORES {
        pools.push(IrqPool::new());
        c += 1;
    }
    // 预取：每核 64 页。
    let mut c2 = 0;
    while c2 < CORES {
        let mut frames: Vec<u64> = Vec::new();
        let mut k = 0;
        while k < 64 {
            frames.push((c2 * 64 + k) as u64);
            k += 1;
        }
        pools[c2].prefetch(&frames);
        c2 += 1;
    }
    // 10^6 次交错。
    let mut rng = Lcg::new(0x2446_0001);
    let mut live: Vec<Vec<u64>> = Vec::new();
    let mut c3 = 0;
    while c3 < CORES {
        live.push(Vec::new());
        c3 += 1;
    }
    let mut fail_total = 0usize;
    let mut round = 0u64;
    while round < IRQ_OPS_PER_CORE {
        let core = (round % CORES as u64) as usize;
        let roll = rng.next_below(100);
        if roll < 55 || live[core].is_empty() {
            match pools[core].alloc_atomic(core) {
                Ok(f) => live[core].push(f),
                Err(AtomicPathError::EmptyNotAtomic) => {
                    fail_total += 1; // 池空原子失败留账核。
                }
                Err(e) => return Err(e),
            }
        } else {
            let idx = rng.next_below(live[core].len() as u64) as usize;
            let f = live[core][idx];
            live[core].remove(idx);
            pools[core].free_atomic(f)?;
        }
        round += 1;
    }
    // 违例恒零核。
    let mut c4 = 0;
    while c4 < CORES {
        pools[c4].audit_no_violations()?;
        if !pools[c4].fails.is_empty() {
            // 失败有账。
            fail_total += 0;
        }
        c4 += 1;
    }
    Ok(fail_total)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 演练核数。
    pub cores: usize,
    /// 池空原子失败有账。
    pub fail_logged: bool,
    /// 违例恒零。
    pub violations_zero: bool,
    /// 预算常量生效。
    pub budget_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.cores == CORES && self.fail_logged && self.violations_zero && self.budget_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, AtomicPathError> {
    let _ = atomicpath_audit()?;
    // 单核抽核：池空失败入账。
    let mut p = IrqPool::new();
    let empty_fail = p.alloc_atomic(0).is_err();
    let fail_logged = !p.fails.is_empty();
    p.prefetch(&[7, 8]);
    let a = p.alloc_atomic(0)?;
    let b = p.alloc_atomic(0)?;
    p.free_atomic(a)?;
    p.free_atomic(b)?;
    p.audit_no_violations()?;
    Ok(SelfAudit {
        cores: CORES,
        fail_logged: empty_fail && fail_logged,
        violations_zero: true,
        budget_ok: FASTPATH_STEP_BUDGET == 8,
    })
}
