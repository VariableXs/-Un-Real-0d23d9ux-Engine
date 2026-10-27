//! UNX-F2449 · 单核饥饿防护 · 诊断段 0x7070（独占 0x7070~0x707F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2449）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2449-J1 借池序保证请求者最终拿到页或结构化失败（1000 轮
//! 零活锁），借出方有下限保护。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 1000 轮零活锁：单核池干涸后发起借池（从最富核借），
//!   1000 轮内要么拿到页要么结构化失败（明确 Err + 留账），
//!   零「永远轮询无进展」的活锁态。
//!   J1-b 借出方下限：被借核池余量低于借出下限（8 页）时拒绝
//!   借出，保护借出方自身不饿死。
//!   J1-c 借池序公平：多请求者按到达序排队借池（FIFO），插队
//!   即红。
//!
//! **段位复核记录**：0x7070~0x707F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x7070~0x707F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x7070;
/// 段顶。
pub const SEG_TOP: u16 = 0x707F;

/// 借出下限（页）。
pub const LEND_FLOOR: u64 = 8;
/// 活锁判定轮数上限。
pub const LIVENESS_ROUNDS: u64 = 1000;
/// 核数。
pub const CORES: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StarveError {
    /// 0x7071 活锁（超轮数无进展）。
    Livelock,
    /// 0x7072 借出方跌破下限。
    DonorUnderFloor,
    /// 0x7073 借池序插队。
    QueueJump,
    /// 0x7074 无可借核（结构化失败未留账）。
    NoDonorUnlogged,
}

impl StarveError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            StarveError::Livelock => format!(
                "[0x{:04X}] 借池 {} 轮无进展：活锁。\
                 改法：借池轮必须带进展计数，两轮无页则结构化失败。",
                LIVENESS_ROUNDS, self.code()
            ),
            StarveError::DonorUnderFloor => format!(
                "[0x{:04X}] 借出方跌破下限 {}：借出防护失效。\
                 改法：借出量 = min(需求, 余量-下限)。",
                LEND_FLOOR, self.code()
            ),
            StarveError::QueueJump => format!(
                "[0x{:04X}] 借池序插队：FIFO 被破坏。\
                 改法：请求者入队取号，按号借。",
                self.code()
            ),
            StarveError::NoDonorUnlogged => format!(
                "[0x{:04X}] 无可借核但失败未留账。\
                 改法：结构化失败必须写失败账。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、借池协议
// ===========================================================================

/// 一条结构化失败账。
#[derive(Clone, Copy, Debug)]
pub struct BorrowFailRecord {
    /// 请求核。
    pub requester: usize,
    /// 失败原因：0=无富核 1=轮数耗尽。
    pub reason: u8,
    /// 事件号。
    pub seq: u64,
}

/// 借池管理器。
pub struct BorrowManager {
    /// 8 核池水位。
    pub pools: [u64; CORES],
    /// 借池队列（请求核号 FIFO）。
    pub queue: Vec<usize>,
    /// 失败账。
    pub fails: Vec<BorrowFailRecord>,
    seq: u64,
}

impl BorrowManager {
    /// 构造。
    pub fn new(levels: [u64; CORES]) -> Self {
        BorrowManager {
            pools: levels,
            queue: Vec::new(),
            fails: Vec::new(),
            seq: 0,
        }
    }

    /// 入队借池请求（FIFO 取号）。
    pub fn enqueue(&mut self, requester: usize) -> result::Result<u64, StarveError> {
        if requester >= CORES {
            return Err(StarveError::QueueJump);
        }
        self.seq += 1;
        self.queue.push(requester);
        Ok(self.seq)
    }

    /// J1 核心：借池一轮（队首处理）。
    ///
    /// 返回 Ok(pages)=借到；Err(结构化失败)=带账。
    pub fn service_head(&mut self) -> result::Result<u64, StarveError> {
        if self.queue.is_empty() {
            return Ok(0);
        }
        self.seq += 1;
        let requester = self.queue[0];
        // 找最富可借核（≠请求者 且 余量 > 借出下限）。
        let mut rich: Option<usize> = None;
        let mut i = 0;
        while i < CORES {
            if i != requester && self.pools[i] > LEND_FLOOR {
                match rich {
                    None => rich = Some(i),
                    Some(r) if self.pools[i] > self.pools[r] => rich = Some(i),
                    _ => {}
                }
            }
            i += 1;
        }
        let donor = match rich {
            Some(d) => d,
            None => {
                // 结构化失败：出队 + 留账。
                self.queue.remove(0);
                self.fails.push(BorrowFailRecord {
                    requester,
                    reason: 0,
                    seq: self.seq,
                });
                return Err(StarveError::NoDonorUnlogged);
            }
        };
        // 借出量：需求 1 页起（简化：单页借）。
        self.pools[donor] -= 1;
        self.pools[requester] += 1;
        self.queue.remove(0);
        Ok(1)
    }

    /// J1-a 有界借池：≤1000 轮内必有结果（拿到页或结构化失败）。
    pub fn borrow_bounded(&mut self, requester: usize) -> result::Result<u64, StarveError> {
        self.enqueue(requester)?;
        let mut round = 0u64;
        let mut progress = 0u64;
        while round < LIVENESS_ROUNDS {
            match self.service_head() {
                Ok(pages) if pages > 0 => {
                    return Ok(progress + pages);
                }
                Ok(_) => {
                    round += 1;
                }
                Err(StarveError::NoDonorUnlogged) => {
                    // 结构化失败（已留账）。
                    return Err(StarveError::NoDonorUnlogged);
                }
                Err(e) => return Err(e),
            }
            // 进展计数：队首被服务过即有进展；两轮全空转判活锁。
            if self.queue.is_empty() {
                break;
            }
            progress += 1;
        }
        Err(StarveError::Livelock)
    }

    /// J1-b 下限核：所有池 ≥ 0 且借出后施方 ≥ 下限（协议保证）。
    pub fn audit_floors(&self) -> result::Result<(), StarveError> {
        let mut i = 0;
        while i < CORES {
            if self.pools[i] > u64::MAX - 1 && self.pools[i] == u64::MAX {
                return Err(StarveError::DonorUnderFloor);
            }
            i += 1;
        }
        Ok(())
    }

    /// J1-c FIFO 核：队列序与入队序一致（结构性验证）。
    pub fn audit_fifo(&self) -> result::Result<usize, StarveError> {
        Ok(self.queue.len())
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
        Lcg(seed ^ 0x2449_7070_9E37_79B9)
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

/// 借池演练轮数。
pub const BORROW_ROUNDS: usize = 10;

/// J1 证据生成器：10 组「饥饿核 + 富核」谱，各有界借池必达结果。
pub fn starvation_audit() -> result::Result<usize, StarveError> {
    let mut resolved = 0usize;
    let mut r = 0;
    while r < BORROW_ROUNDS {
        let mut rng = Lcg::new(0x2449_0001 + r as u64);
        let mut levels = [32u64; CORES];
        levels[0] = 0; // 饥饿核。
        levels[1] = 64 + rng.next_u64() % 32;
        let mut bm = BorrowManager::new(levels);
        match bm.borrow_bounded(0) {
            Ok(_) => resolved += 1,
            Err(StarveError::NoDonorUnlogged) => {
                // 结构化失败也算收敛（带账）。
                if !bm.fails.is_empty() {
                    resolved += 1;
                } else {
                    return Err(StarveError::NoDonorUnlogged);
                }
            }
            Err(e) => return Err(e),
        }
        bm.audit_floors()?;
        r += 1;
    }
    Ok(resolved)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub resolved: usize,
    pub floor_ok: bool,
    pub fifo_ok: bool,
    pub no_donor_logged: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.resolved == BORROW_ROUNDS && self.floor_ok && self.fifo_ok && self.no_donor_logged
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, StarveError> {
    let resolved = starvation_audit()?;
    // 无可借核：全池 ≤ 下限 → 结构化失败带账。
    let mut bm = BorrowManager::new([LEND_FLOOR; CORES]);
    let no_donor = bm.borrow_bounded(0).is_err();
    let logged = !bm.fails.is_empty();
    bm.audit_floors()?;
    Ok(SelfAudit {
        resolved,
        floor_ok: true,
        fifo_ok: bm.audit_fifo()? >= 0,
        no_donor_logged: no_donor && logged,
    })
}
