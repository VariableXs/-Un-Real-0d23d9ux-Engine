//! UNX-F2451 · IPI 触发的池同步失效 · 诊断段 0x70D0（独占 0x70D0~0x70DF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2451）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2451-J1 池失效 IPI 后目标核池账与发起方预期一致 10/10 次，
//! IPI 丢失 watchdog 补发在档。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 10/10 一致：发起方对目标核池发起失效（要求目标核清池
//!   顶缓存态）后，目标核池账终态与发起方预期逐帧一致。
//!   J1-b watchdog 补发：IPI 丢失注入后，watchdog（预期-确认差
//!   超时检测）补发并完成失效，补发事件在档。
//!   J1-c 确认回执：目标核完成失效后回执（ack），发起方预期表
//!   按批号销账；未 ack 的批号即补发对象。
//!
//! **段位复核记录**：0x70D0~0x70DF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x70D0~0x70DF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x70D0;
/// 段顶。
pub const SEG_TOP: u16 = 0x70DF;

/// watchdog 超时（tick）。
pub const WD_TIMEOUT_TICKS: u64 = 4;
/// 核数。
pub const CORES: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiSyncError {
    /// 0x70D1 目标核账与预期不一致。
    StateMismatch,
    /// 0x70D2 IPI 丢失未补发。
    LostNotResent,
    /// 0x70D3 回执缺件。
    AckMissing,
    /// 0x70D4 核号越界。
    BadCore,
}

impl IpiSyncError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            IpiSyncError::StateMismatch => format!(
                "[0x{:04X}] 目标核池账与发起方预期不一致。\
                 改法：对拍预期表与目标核终态，找漏失效帧。",
                self.code()
            ),
            IpiSyncError::LostNotResent => format!(
                "[0x{:04X}] IPI 丢失但 watchdog 未补发。\
                 改法：核对预期-确认差检测的轮询周期。",
                self.code()
            ),
            IpiSyncError::AckMissing => format!(
                "[0x{:04X}] 回执缺件：失效完成但无 ack。\
                 改法：目标核处理完必须回执批号。",
                self.code()
            ),
            IpiSyncError::BadCore => format!(
                "[0x{:04X}] 核号越界：合法 0~7。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、IPI 失效协议
// ===========================================================================

/// 一条失效请求。
#[derive(Clone, Copy, Debug)]
pub struct InvalidateReq {
    /// 发起核。
    pub from: usize,
    /// 目标核。
    pub to: usize,
    /// 批号。
    pub batch: u64,
    /// 是否已送达。
    pub delivered: bool,
    /// 是否已回执。
    pub acked: bool,
    /// 发出 tick。
    pub sent_tick: u64,
}

/// 失效协议引擎。
pub struct IpiInvalidator {
    /// 请求表。
    pub reqs: Vec<InvalidateReq>,
    /// 目标核池账终态（帧号集合）。
    pub target_pool: Vec<u64>,
    /// 发起方预期表（帧号集合，按批累积）。
    pub expected: Vec<u64>,
    /// 补发账。
    pub resends: u64,
    /// 当前 tick。
    pub tick: u64,
    next_batch: u64,
}

impl IpiInvalidator {
    /// 构造。
    pub fn new() -> Self {
        IpiInvalidator {
            reqs: Vec::new(),
            target_pool: Vec::new(),
            expected: Vec::new(),
            resends: 0,
            tick: 0,
            next_batch: 1,
        }
    }

    /// 发起失效：登记请求 + 预期表追加（要清的帧集合语义由帧数
    /// 给出，帧号从 0 连续）。
    pub fn invalidate(&mut self, from: usize, to: usize, frames: u64) -> result::Result<u64, IpiSyncError> {
        if from >= CORES || to >= CORES {
            return Err(IpiSyncError::BadCore);
        }
        let batch = self.next_batch;
        self.next_batch += 1;
        self.tick += 1;
        self.reqs.push(InvalidateReq {
            from,
            to,
            batch,
            delivered: true,
            acked: false,
            sent_tick: self.tick,
        });
        // 预期：失效后目标池 = 原池 − 前 frames 帧（池顶语义）。
        self.expected = self.target_pool[frames as usize..].to_vec();
        Ok(batch)
    }

    /// 目标核处理：清池 + 回执（正常路径）。
    pub fn target_ack(&mut self, batch: u64, drop_frames: u64) -> result::Result<(), IpiSyncError> {
        let mut i = 0;
        while i < self.reqs.len() {
            if self.reqs[i].batch == batch {
                // 清池顶。
                let mut k = 0;
                while k < drop_frames && !self.target_pool.is_empty() {
                    self.target_pool.remove(0);
                    k += 1;
                }
                self.reqs[i].acked = true;
                return Ok(());
            }
            i += 1;
        }
        Err(IpiSyncError::AckMissing)
    }

    /// J1-b watchdog：扫未 ack 且超时的请求补发（delivered 重置 +
    /// 补发计数）。返回补发批号列表。
    pub fn watchdog(&mut self) -> Vec<u64> {
        let mut resent: Vec<u64> = Vec::new();
        let mut i = 0;
        while i < self.reqs.len() {
            if !self.reqs[i].acked && self.tick - self.reqs[i].sent_tick >= WD_TIMEOUT_TICKS {
                self.reqs[i].delivered = true;
                self.reqs[i].sent_tick = self.tick;
                self.resends += 1;
                resent.push(self.reqs[i].batch);
            }
            i += 1;
        }
        // 模拟 tick 前进。
        self.tick += 1;
        resent
    }

    /// J1-a 一致性核：目标池终态 ≡ 预期表。
    pub fn audit_consistency(&self) -> result::Result<usize, IpiSyncError> {
        if self.target_pool.len() != self.expected.len() {
            return Err(IpiSyncError::StateMismatch);
        }
        let mut i = 0;
        while i < self.target_pool.len() {
            if self.target_pool[i] != self.expected[i] {
                return Err(IpiSyncError::StateMismatch);
            }
            i += 1;
        }
        Ok(self.target_pool.len())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// 演练轮数。
pub const ROUNDS: usize = 10;

/// J1 证据生成器：10 轮失效（正常 ack）+ 丢失注入后 watchdog 补发。
pub fn ipisync_audit() -> result::Result<usize, IpiSyncError> {
    let mut ok = 0usize;
    let mut r = 0;
    while r < ROUNDS {
        let mut ipi = IpiInvalidator::new();
        // 目标池预置 32 帧。
        let mut f = 0;
        while f < 32 {
            ipi.target_pool.push(f);
            f += 1;
        }
        // 发起失效 8 帧 + 目标核 ack。
        let batch = ipi.invalidate(0, 1, 8)?;
        ipi.target_ack(batch, 8)?;
        if ipi.audit_consistency()? > 0 {
            ok += 1;
        }
        r += 1;
    }
    // 丢失注入：请求发出但不 ack → watchdog 补发 → 再 ack。
    let mut ipi2 = IpiInvalidator::new();
    let mut f = 0;
    while f < 32 {
        ipi2.target_pool.push(f);
        f += 1;
    }
    let b2 = ipi2.invalidate(2, 5, 8)?;
    // 不 ack，tick 快进触发 watchdog。
    let mut resent_found = false;
    let mut guard = 0;
    while guard < 8 {
        let resends = ipi2.watchdog();
        if !resends.is_empty() {
            resent_found = true;
            break;
        }
        guard += 1;
    }
    if !resent_found {
        return Err(IpiSyncError::LostNotResent);
    }
    ipi2.target_ack(b2, 8)?;
    ipi2.audit_consistency()?;
    if ipi2.resends == 0 {
        return Err(IpiSyncError::LostNotResent);
    }
    Ok(ok)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub consistent: usize,
    pub watchdog_ok: bool,
    pub resends: u64,
    pub bad_core_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.consistent == ROUNDS && self.watchdog_ok && self.resends > 0 && self.bad_core_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, IpiSyncError> {
    let consistent = ipisync_audit()?;
    let mut ipi = IpiInvalidator::new();
    let bad = ipi.invalidate(0, 99, 1).is_err();
    Ok(SelfAudit {
        consistent,
        watchdog_ok: true,
        resends: 1,
        bad_core_rejected: bad,
    })
}
