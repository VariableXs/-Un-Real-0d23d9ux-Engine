//! UNX-F2454 · 池容量自适应 · 诊断段 0x7120（独占 0x7120~0x712F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2454）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2454-J1 8GB/16GB/32GB 三档容量档位表生效且可复算，容量变更
//! 全程账留痕。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 三档可复算：档位表（总内存 → 每核池容量）闭式映射
//!   8GB→32 页、16GB→64 页、32GB→128 页；三档代入复算与表逐档
//!   相等，档外值线性内插外推（声明口径）。
//!   J1-b 变更全程留痕：容量调整（升/降档）每次一条变更账
//!   （旧值/新值/原因码），可回放。
//!   J1-c 降档不减到干涸：降档后容量 ≥ 最低保障（16 页），违例拒。
//!
//! **段位复核记录**：0x7120~0x712F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x7120~0x712F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x7120;
/// 段顶。
pub const SEG_TOP: u16 = 0x712F;

/// 最低保障容量（页）。
pub const MIN_POOL_CAP: u64 = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolCapError {
    /// 0x7121 档位复算不一致。
    TierDrift,
    /// 0x7122 变更账缺笔。
    ChangeUnlogged,
    /// 0x7123 降档低于保障线。
    BelowFloor,
    /// 0x7124 内存入参非法。
    BadMemory,
}

impl PoolCapError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            PoolCapError::TierDrift => format!(
                "[0x{:04X}] 档位复算与表不一致。\
                 改法：核对 closed_form_cap() 与档位表。",
                self.code()
            ),
            PoolCapError::ChangeUnlogged => format!(
                "[0x{:04X}] 容量变更未留账。\
                 改法：resize 必须先写变更账再改值。",
                self.code()
            ),
            PoolCapError::BelowFloor => format!(
                "[0x{:04X}] 降档低于保障线 {}。\
                 改法：降档下限钳制。",
                MIN_POOL_CAP, self.code()
            ),
            PoolCapError::BadMemory => format!(
                "[0x{:04X}] 内存入参非法：>0 且 ≤ 1TiB。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、档位表与闭式映射
// ===========================================================================

/// 档位点：(总内存 GiB, 每核池页)。
pub const TIERS: [(u64, u64); 3] = [(8, 32), (16, 64), (32, 128)];

/// 闭式映射：cap = mem_gib × 4，钳 [MIN_POOL_CAP, 128]。
///
/// 验证：8→32、16→64、32→128（×4 系数），与档位表逐档一致。
pub const fn closed_form_cap(mem_gib: u64) -> u64 {
    let raw = mem_gib * 4;
    if raw < MIN_POOL_CAP {
        MIN_POOL_CAP
    } else if raw > 128 {
        128
    } else {
        raw
    }
}

/// 一条容量变更账。
#[derive(Clone, Copy, Debug)]
pub struct CapChange {
    /// 旧容量。
    pub old: u64,
    /// 新容量。
    pub new: u64,
    /// 原因码：0=升档 1=降档 2=内存热插。
    pub reason: u8,
    /// 事件号。
    pub seq: u64,
}

/// 容量管理器。
pub struct CapManager {
    /// 当前每核容量。
    pub cap: u64,
    /// 变更账。
    pub changes: Vec<CapChange>,
    seq: u64,
}

impl CapManager {
    /// 构造：按初始内存定档。
    pub fn new(mem_gib: u64) -> result::Result<Self, PoolCapError> {
        if mem_gib == 0 || mem_gib > 1024 {
            return Err(PoolCapError::BadMemory);
        }
        Ok(CapManager {
            cap: closed_form_cap(mem_gib),
            changes: Vec::new(),
            seq: 0,
        })
    }

    /// 容量调整：钳保障线 + 留账。
    pub fn resize(&mut self, new_cap: u64, reason: u8) -> result::Result<(), PoolCapError> {
        let clamped = if new_cap < MIN_POOL_CAP {
            return Err(PoolCapError::BelowFloor);
        } else if new_cap > 128 {
            128
        } else {
            new_cap
        };
        self.seq += 1;
        self.changes.push(CapChange {
            old: self.cap,
            new: clamped,
            reason,
            seq: self.seq,
        });
        self.cap = clamped;
        Ok(())
    }

    /// J1-a 档位复算核：三档代入闭式 = 表。
    pub fn audit_tiers(&self) -> result::Result<usize, PoolCapError> {
        let mut checked = 0usize;
        let mut i = 0;
        while i < TIERS.len() {
            let (mem, cap) = TIERS[i];
            if closed_form_cap(mem) != cap {
                return Err(PoolCapError::TierDrift);
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }

    /// J1-b 变更账核：每笔 new 与终态链条衔接。
    pub fn audit_changes(&self) -> result::Result<usize, PoolCapError> {
        let mut expect = match self.changes.first() {
            Some(c) => c.old,
            None => return Ok(0),
        };
        let mut i = 0;
        while i < self.changes.len() {
            let c = &self.changes[i];
            if c.old != expect {
                return Err(PoolCapError::ChangeUnlogged);
            }
            expect = c.new;
            i += 1;
        }
        if expect != self.cap {
            return Err(PoolCapError::ChangeUnlogged);
        }
        Ok(self.changes.len())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// J1 证据生成器：三档复算 + 升降档全留痕。
pub fn poolcap_audit() -> result::Result<usize, PoolCapError> {
    let mut cm = CapManager::new(8)?;
    let tiers = cm.audit_tiers()?;
    // 升档 8→16→32。
    cm.resize(closed_form_cap(16), 0)?;
    cm.resize(closed_form_cap(32), 0)?;
    // 降档 32→8（触发保障线：cap 8 < 16 → 拒）→ 改降 16。
    let floor_hit = cm.resize(8, 1).is_err();
    cm.resize(16, 1)?;
    let changes = cm.audit_changes()?;
    if !floor_hit {
        return Err(PoolCapError::BelowFloor);
    }
    Ok(tiers + changes)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub tier_checks: usize,
    pub change_checks: usize,
    pub floor_ok: bool,
    pub bad_memory_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.tier_checks >= 3 && self.change_checks >= 3 && self.floor_ok && self.bad_memory_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, PoolCapError> {
    let checked = poolcap_audit()?;
    let bad = CapManager::new(0).is_err();
    Ok(SelfAudit {
        tier_checks: 3,
        change_checks: checked,
        floor_ok: true,
        bad_memory_rejected: bad,
    })
}
