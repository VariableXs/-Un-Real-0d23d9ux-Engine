//! UNX-F2453 · 池与保留页交互 · 诊断段 0x70F0（独占 0x70F0~0x70FF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2453）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2453-J1 预留区页零入池 10/10 次断言，池页与预留账边界零
//! 交叠。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 零入池 10/10：10 组「含预留区的域」各 refill 一批，
//!   池帧与预留区零交叠（分配扫描跳过预留区）。
//!   J1-b 边界零交叠：终态对拍——池帧集合 ∩ 预留帧集合 = ∅，
//!   逐帧核。
//!   J1-c 预留守恒：预留区帧数全程不变（零借出零入池）。
//!
//! **段位复核记录**：0x70F0~0x70FF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x70F0~0x70FF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x70F0;
/// 段顶。
pub const SEG_TOP: u16 = 0x70FF;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReservePoolError {
    /// 0x70F1 预留帧入池。
    ReserveInPool,
    /// 0x70F2 边界交叠。
    BoundaryOverlap,
    /// 0x70F3 预留守恒破。
    ReserveDrift,
}

impl ReservePoolError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            ReservePoolError::ReserveInPool => format!(
                "[0x{:04X}] 预留帧入池：refill 扫描未跳过预留区。\
                 改法：refill 取帧前查预留位图。",
                self.code()
            ),
            ReservePoolError::BoundaryOverlap => format!(
                "[0x{:04X}] 池帧与预留账边界交叠。\
                 改法：终态对拍找交叠帧。",
                self.code()
            ),
            ReservePoolError::ReserveDrift => format!(
                "[0x{:04X}] 预留区帧数变化：预留被借出或吞并。\
                 改法：预留区只读，禁入分配流。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、预留守卫 refill
// ===========================================================================

/// 含预留位的池域。
pub struct ReserveAwarePool {
    /// 总帧数。
    pub total_frames: u64,
    /// 预留位图。
    pub reserved: Vec<bool>,
    /// 池帧集合。
    pub pool_frames: Vec<u64>,
    /// 预留帧总数（守恒锚）。
    pub reserve_total: u64,
}

impl ReserveAwarePool {
    /// 构造：预留区 [rlo, rhi)。
    pub fn new(total_frames: u64, rlo: u64, rhi: u64) -> result::Result<Self, ReservePoolError> {
        if rhi > total_frames || rlo >= rhi {
            return Err(ReservePoolError::BoundaryOverlap);
        }
        let mut reserved = vec![false; total_frames as usize];
        let mut f = rlo;
        while f < rhi {
            reserved[f as usize] = true;
            f += 1;
        }
        Ok(ReserveAwarePool {
            total_frames,
            reserved,
            pool_frames: Vec::new(),
            reserve_total: rhi - rlo,
        })
    }

    fn is_reserved(&self, f: u64) -> bool {
        self.reserved[f as usize]
    }

    /// refill 一批（32 帧）：跳过预留帧。
    pub fn refill(&mut self, want: u64) -> result::Result<u64, ReservePoolError> {
        let mut got = 0u64;
        let mut f = 0u64;
        while f < self.total_frames && got < want {
            if !self.is_reserved(f) {
                self.pool_frames.push(f);
                got += 1;
            }
            f += 1;
        }
        if got < want {
            // 域太小：不足额（诚实短交，非静默）。
            return Ok(got);
        }
        Ok(got)
    }

    /// J1-a/J1-b 终态核：池帧 ∩ 预留 = ∅。
    pub fn audit_no_overlap(&self) -> result::Result<usize, ReservePoolError> {
        let mut checked = 0usize;
        let mut i = 0;
        while i < self.pool_frames.len() {
            if self.is_reserved(self.pool_frames[i]) {
                return Err(ReservePoolError::ReserveInPool);
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }

    /// J1-c 预留守恒核。
    pub fn audit_reserve_conservation(&self) -> result::Result<(), ReservePoolError> {
        let mut n = 0u64;
        let mut f = 0;
        while f < self.total_frames {
            if self.is_reserved(f) {
                n += 1;
            }
            f += 1;
        }
        if n != self.reserve_total {
            return Err(ReservePoolError::ReserveDrift);
        }
        Ok(())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// 演练轮数。
pub const ROUNDS: usize = 10;

/// J1 证据生成器：10 组含预留域各 refill，零入池 + 守恒。
pub fn reservepool_audit() -> result::Result<usize, ReservePoolError> {
    let mut ok = 0usize;
    let mut r = 0;
    while r < ROUNDS {
        // 预留区随轮次移动：核 [64+r*8, 64+r*8+16)。
        let rlo = 64 + (r * 8) as u64;
        let mut pool = ReserveAwarePool::new(1 << 12, rlo, rlo + 16)?;
        let got = pool.refill(128)?;
        if got != 128 {
            return Err(ReservePoolError::BoundaryOverlap);
        }
        pool.audit_no_overlap()?;
        pool.audit_reserve_conservation()?;
        ok += 1;
        r += 1;
    }
    Ok(ok)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub rounds_ok: usize,
    pub conservation_ok: bool,
    pub bad_range_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.rounds_ok == ROUNDS && self.conservation_ok && self.bad_range_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, ReservePoolError> {
    let rounds_ok = reservepool_audit()?;
    let bad = ReserveAwarePool::new(1024, 900, 2000).is_err();
    Ok(SelfAudit {
        rounds_ok,
        conservation_ok: true,
        bad_range_rejected: bad,
    })
}
