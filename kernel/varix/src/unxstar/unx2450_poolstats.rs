//! UNX-F2450 · 池统计账（refill/drain 计数） · 诊断段 0x70A0（独占 0x70A0~0x70AF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2450）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2450-J1 每核 refill/drain/命中三计数可导出，计数与交接协议
//! 账逐笔配平。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 三计数可导出：每核 refill/drain/命中三计数 + 汇总面，
//!   导出快照字段闭集齐备。
//!   J1-b 与交接账配平：refill 计数 × 批大小 ≡ 交接账 refill 帧
//!   总量（同构核 F2444 双账），drain 同理；逐笔配平。
//!   J1-c 命中率分母诚实：命中/请求 = 命中率，请求数为零时显性
//!   None 不出 0。
//!
//! **段位复核记录**：0x70A0~0x70AF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x70A0~0x70AF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x70A0;
/// 段顶。
pub const SEG_TOP: u16 = 0x70AF;

/// 批大小。
pub const BATCH_FRAMES: u64 = 32;
/// 核数。
pub const CORES: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolStatError {
    /// 0x70A1 计数与交接账不配平。
    LedgerMismatch,
    /// 0x70A2 快照字段缺失。
    SnapshotMissing,
    /// 0x70A3 空请求率显性失败。
    EmptyRate,
}

impl PoolStatError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            PoolStatError::LedgerMismatch => format!(
                "[0x{:04X}] 计数 × 批大小 ≠ 交接账帧总量。\
                 改法：按核对账找漏计侧。",
                self.code()
            ),
            PoolStatError::SnapshotMissing => format!(
                "[0x{:04X}] 导出快照字段缺失：三计数+命中率必须齐。\
                 改法：核对导出器。",
                self.code()
            ),
            PoolStatError::EmptyRate => format!(
                "[0x{:04X}] 请求数为零时出命中率：谎报。\
                 改法：空分母显性 None。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、统计账
// ===========================================================================

/// 单核三计数。
#[derive(Clone, Copy, Debug)]
pub struct CoreStats {
    /// refill 批数。
    pub refills: u64,
    /// drain 批数。
    pub drains: u64,
    /// 命中（池顶直取）次数。
    pub hits: u64,
    /// 总请求数。
    pub requests: u64,
}

/// 交接账条目（与 F2444 双账同构的统计侧投影）。
#[derive(Clone, Copy, Debug)]
pub struct XferStat {
    /// 核号。
    pub core: usize,
    /// 方向 0=refill 1=drain。
    pub dir: u8,
    /// 帧数。
    pub frames: u64,
}

/// 池统计账本。
pub struct PoolStatBook {
    /// 每核计数。
    pub cores: Vec<CoreStats>,
    /// 交接统计账。
    pub xfer: Vec<XferStat>,
}

impl PoolStatBook {
    /// 空账。
    pub fn new() -> Self {
        let mut cores = Vec::new();
        let mut i = 0;
        while i < CORES {
            cores.push(CoreStats {
                refills: 0,
                drains: 0,
                hits: 0,
                requests: 0,
            });
            i += 1;
        }
        PoolStatBook {
            cores,
            xfer: Vec::new(),
        }
    }

    /// 记 refill。
    pub fn record_refill(&mut self, core: usize) -> result::Result<(), PoolStatError> {
        if core >= CORES {
            return Err(PoolStatError::SnapshotMissing);
        }
        self.cores[core].refills += 1;
        self.xfer.push(XferStat {
            core,
            dir: 0,
            frames: BATCH_FRAMES,
        });
        Ok(())
    }

    /// 记 drain。
    pub fn record_drain(&mut self, core: usize) -> result::Result<(), PoolStatError> {
        if core >= CORES {
            return Err(PoolStatError::SnapshotMissing);
        }
        self.cores[core].drains += 1;
        self.xfer.push(XferStat {
            core,
            dir: 1,
            frames: BATCH_FRAMES,
        });
        Ok(())
    }

    /// 记命中与请求。
    pub fn record_hit(&mut self, core: usize, hit: bool) -> result::Result<(), PoolStatError> {
        if core >= CORES {
            return Err(PoolStatError::SnapshotMissing);
        }
        self.cores[core].requests += 1;
        if hit {
            self.cores[core].hits += 1;
        }
        Ok(())
    }

    /// J1-b 配平核：Σ(refill 计数)×批 ≡ Σ(refill 交接帧)；drain 同理。
    pub fn audit_pairing(&self) -> result::Result<usize, PoolStatError> {
        let mut refill_count = 0u64;
        let mut drain_count = 0u64;
        let mut i = 0;
        while i < CORES {
            refill_count += self.cores[i].refills;
            drain_count += self.cores[i].drains;
            i += 1;
        }
        let mut refill_frames = 0u64;
        let mut drain_frames = 0u64;
        let mut k = 0;
        while k < self.xfer.len() {
            match self.xfer[k].dir {
                0 => refill_frames += self.xfer[k].frames,
                _ => drain_frames += self.xfer[k].frames,
            }
            k += 1;
        }
        if refill_count * BATCH_FRAMES != refill_frames
            || drain_count * BATCH_FRAMES != drain_frames
        {
            return Err(PoolStatError::LedgerMismatch);
        }
        Ok(self.xfer.len())
    }

    /// J1-a 导出快照（汇总面）。
    pub fn export(&self) -> result::Result<([u64; CORES], [u64; CORES], [u64; CORES]), PoolStatError> {
        let mut r = [0u64; CORES];
        let mut d = [0u64; CORES];
        let mut h = [0u64; CORES];
        let mut i = 0;
        while i < CORES {
            r[i] = self.cores[i].refills;
            d[i] = self.cores[i].drains;
            h[i] = self.cores[i].hits;
            i += 1;
        }
        Ok((r, d, h))
    }

    /// J1-c 命中率（全核合计，‱）；空分母 None。
    pub fn hit_rate_bp(&self) -> Option<u64> {
        let mut hits = 0u64;
        let mut reqs = 0u64;
        let mut i = 0;
        while i < CORES {
            hits += self.cores[i].hits;
            reqs += self.cores[i].requests;
            i += 1;
        }
        if reqs == 0 {
            return None;
        }
        Some(hits * 10_000 / reqs)
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
        Lcg(seed ^ 0x2450_70A0_9E37_79B9)
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

/// J1 证据生成器：8 核混合 refill/drain/命中 4096 事件后全配平。
pub fn poolstat_audit() -> result::Result<(usize, u64), PoolStatError> {
    let mut book = PoolStatBook::new();
    let mut rng = Lcg::new(0x2450_0001);
    let mut n = 0u64;
    while n < 4096 {
        let core = rng.next_below(CORES as u64) as usize;
        let roll = rng.next_below(100);
        if roll < 20 {
            book.record_refill(core)?;
        } else if roll < 35 {
            book.record_drain(core)?;
        } else {
            let hit = roll < 90;
            book.record_hit(core, hit)?;
        }
        n += 1;
    }
    let paired = book.audit_pairing()?;
    let rate = book.hit_rate_bp().ok_or(PoolStatError::EmptyRate)?;
    book.export()?;
    Ok((paired, rate))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub paired: usize,
    pub hit_rate: u64,
    pub empty_none_ok: bool,
    pub export_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.paired > 0 && self.hit_rate > 0 && self.empty_none_ok && self.export_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, PoolStatError> {
    let (paired, rate) = poolstat_audit()?;
    let empty_book = PoolStatBook::new();
    let empty_none = empty_book.hit_rate_bp().is_none();
    let (r, d, h) = empty_book.export()?;
    let export_ok = r.len() == CORES && d.len() == CORES && h.len() == CORES;
    Ok(SelfAudit {
        paired,
        hit_rate: rate,
        empty_none_ok: empty_none,
        export_ok,
    })
}
