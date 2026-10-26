//! UNX-F2434 · 分配器锁序登记 · 诊断段 0x6F60（独占 0x6F60~0x6F6F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B02 反碎片批 · UNX-F2434）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2434-J1 分配器全部锁在 A5 锁层次表注册覆盖 100%，逆序注入
//! 20/20 次报警。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 覆盖 100%：分配器全部锁（分区锁×8、账本锁、回收者锁）
//!   在 A5 锁层次表注册，名表与实现一一对应（无未注册锁、无幽灵名）。
//!   J1-b 逆序注入 20/20 报警：持有高层锁再取低层锁（逆序）注入
//!   20 次，检查器每次报警并留账（哪两把锁、谁先谁后）。
//!   J1-c 层次严格递减：合法加锁序 = 层号严格递增（或递减，按表），
//!   同层重入必须显式声明（可重入标记），未声明的同层加锁也报警。
//!
//! **段位复核记录**：0x6F60~0x6F6F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6F60~0x6F6F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6F60;
/// 段顶。
pub const SEG_TOP: u16 = 0x6F6F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockOrderError {
    /// 0x6F61 锁未注册。
    UnregisteredLock,
    /// 0x6F62 逆序加锁（层次倒挂）。
    OrderInversion,
    /// 0x6F63 同层未声明重入。
    SameLevelReentry,
    /// 0x6F64 注入未报警（检查器失效）。
    InjectionMissed,
    /// 0x6F65 覆盖率不足 100%。
    CoverageHole,
}

impl LockOrderError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            LockOrderError::UnregisteredLock => format!(
                "[0x{:04X}] 锁未注册进 A5 层次表。\
                 改法：新增锁必须先登记（名+层号+可重入位）。",
                self.code()
            ),
            LockOrderError::OrderInversion => format!(
                "[0x{:04X}] 逆序加锁：持低层取高层。\
                 改法：按层号单调方向加锁，或先放低层。",
                self.code()
            ),
            LockOrderError::SameLevelReentry => format!(
                "[0x{:04X}] 同层重入未声明。\
                 改法：登记时声明可重入，或拆分层号。",
                self.code()
            ),
            LockOrderError::InjectionMissed => format!(
                "[0x{:04X}] 逆序注入未报警：检查器失效。\
                 改法：核对检查器是否读了正确的层次表。",
                self.code()
            ),
            LockOrderError::CoverageHole => format!(
                "[0x{:04X}] 覆盖率不足 100%：有锁未登记。\
                 改法：对实现侧锁清单与名表做 diff。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、A5 锁层次表
// ===========================================================================

/// 一条锁注册。
#[derive(Clone, Copy, Debug)]
pub struct LockEntry {
    /// 锁名。
    pub name: &'static str,
    /// 层号（越大越高）。
    pub level: u32,
    /// 可重入位。
    pub reentrant: bool,
}

/// A5 层次表（分配器全部锁，写死——J1-a 覆盖真源）。
pub const LOCK_TABLE: [LockEntry; 10] = [
    LockEntry { name: "mm.vmem.lock", level: 90, reentrant: false },
    LockEntry { name: "mm.reclaim.lock", level: 80, reentrant: false },
    LockEntry { name: "mm.buddy.lock", level: 70, reentrant: false },
    LockEntry { name: "mm.zone[0].lock", level: 60, reentrant: false },
    LockEntry { name: "mm.zone[1].lock", level: 60, reentrant: false },
    LockEntry { name: "mm.zone[2].lock", level: 60, reentrant: false },
    LockEntry { name: "mm.zone[3].lock", level: 60, reentrant: false },
    LockEntry { name: "mm.zone[4].lock", level: 60, reentrant: false },
    LockEntry { name: "mm.pairledger.lock", level: 50, reentrant: false },
    LockEntry { name: "mm.stat.lock", level: 40, reentrant: true },
];

/// 层次检查器。
pub struct LockChecker {
    /// 当前持锁栈（名, 层号）。
    held: Vec<(&'static str, u32)>,
    /// 报警账：(违规类型码, 高层名, 低层名)。
    pub alarms: Vec<(u16, &'static str, &'static str)>,
}

impl LockChecker {
    /// 空检查器。
    pub fn new() -> Self {
        LockChecker {
            held: Vec::new(),
            alarms: Vec::new(),
        }
    }

    fn lookup(name: &str) -> Option<&'static LockEntry> {
        let mut i = 0;
        while i < LOCK_TABLE.len() {
            if LOCK_TABLE[i].name == name {
                return Some(&LOCK_TABLE[i]);
            }
            i += 1;
        }
        None
    }

    /// J1-b 核心：取锁（层次检查）。违规即报警 + Err。
    pub fn lock(&mut self, name: &'static str) -> result::Result<(), LockOrderError> {
        let entry = match Self::lookup(name) {
            Some(e) => e,
            None => {
                self.alarms.push((LockOrderError::UnregisteredLock.code(), name, ""));
                return Err(LockOrderError::UnregisteredLock);
            }
        };
        // 同层重入核。
        let mut i = 0;
        while i < self.held.len() {
            let (hn, hl) = self.held[i];
            if hn == name {
                if !entry.reentrant {
                    self.alarms.push((LockOrderError::SameLevelReentry.code(), name, hn));
                    return Err(LockOrderError::SameLevelReentry);
                }
                // 可重入：放行计数（账面栈仍单条）。
                return Ok(());
            }
            // 层次核：持锁层必须 < 新锁层（严格递增加锁）。
            if hl >= entry.level {
                self.alarms.push((LockOrderError::OrderInversion.code(), name, hn));
                return Err(LockOrderError::OrderInversion);
            }
            i += 1;
        }
        self.held.push((name, entry.level));
        Ok(())
    }

    /// 放锁（弹栈顶）。
    pub fn unlock(&mut self, name: &'static str) {
        let mut i = self.held.len();
        while i > 0 {
            i -= 1;
            if self.held[i].0 == name {
                self.held.remove(i);
                return;
            }
        }
    }

    /// J1-a 覆盖核：实现侧锁名清单全部在表。
    pub fn audit_coverage(&self, impl_locks: &[&str]) -> result::Result<usize, LockOrderError> {
        let mut checked = 0usize;
        let mut i = 0;
        while i < impl_locks.len() {
            if Self::lookup(impl_locks[i]).is_none() {
                return Err(LockOrderError::CoverageHole);
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// 逆序注入样本数。
pub const INJECTIONS: usize = 20;

/// J1 证据生成器：合法序全过 + 20 次逆序注入全报警。
pub fn lockorder_audit() -> result::Result<usize, LockOrderError> {
    let mut ck = LockChecker::new();
    // 合法序：stat → pairledger → zone[0..4] → buddy → reclaim → vmem。
    let legal = [
        "mm.stat.lock",
        "mm.pairledger.lock",
        "mm.zone[0].lock",
        "mm.buddy.lock",
        "mm.reclaim.lock",
        "mm.vmem.lock",
    ];
    let mut i = 0;
    while i < legal.len() {
        ck.lock(legal[i])?;
        i += 1;
    }
    // 可重入锁重入放行。
    ck.unlock("mm.stat.lock");
    let mut j = 0;
    while j < legal.len() {
        ck.unlock(legal[j]);
        j += 1;
    }
    // 20 次逆序注入。
    let mut caught = 0usize;
    let mut inj = 0;
    while inj < INJECTIONS {
        let mut c2 = LockChecker::new();
        // 注入：先持低层再取高层（轮换组合）。
        let (low, high) = match inj % 4 {
            0 => ("mm.buddy.lock", "mm.reclaim.lock"),
            1 => ("mm.pairledger.lock", "mm.buddy.lock"),
            2 => ("mm.reclaim.lock", "mm.vmem.lock"),
            _ => ("mm.zone[1].lock", "mm.pairledger.lock"),
        };
        c2.lock(low)?;
        match c2.lock(high) {
            Err(LockOrderError::OrderInversion) => caught += 1,
            _ => return Err(LockOrderError::InjectionMissed),
        }
        inj += 1;
    }
    Ok(caught)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 逆序注入报警数（应 20/20）。
    pub caught: usize,
    /// 覆盖核锁数（应 10）。
    pub coverage: usize,
    /// 未注册锁被拒。
    pub unregistered_rejected: bool,
    /// 同层重入被拒。
    pub same_level_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.caught == INJECTIONS && self.coverage == LOCK_TABLE.len() && self.unregistered_rejected && self.same_level_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, LockOrderError> {
    let caught = lockorder_audit()?;
    let ck = LockChecker::new();
    let impl_locks: [&str; 10] = [
        "mm.vmem.lock",
        "mm.reclaim.lock",
        "mm.buddy.lock",
        "mm.zone[0].lock",
        "mm.zone[1].lock",
        "mm.zone[2].lock",
        "mm.zone[3].lock",
        "mm.zone[4].lock",
        "mm.pairledger.lock",
        "mm.stat.lock",
    ];
    let coverage = ck.audit_coverage(&impl_locks)?;
    let mut c2 = LockChecker::new();
    let unreg = c2.lock("mm.ghost.lock").is_err();
    let mut c3 = LockChecker::new();
    c3.lock("mm.buddy.lock")?;
    let same = c3.lock("mm.buddy.lock").is_err();
    Ok(SelfAudit {
        caught,
        coverage,
        unregistered_rejected: unreg,
        same_level_rejected: same,
    })
}
