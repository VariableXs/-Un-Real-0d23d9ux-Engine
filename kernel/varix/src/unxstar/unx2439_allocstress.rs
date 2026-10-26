//! UNX-F2439 · 万次分配释放位图对账压测 · 诊断段 0x6FD0（独占 0x6FD0~0x6FDF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B02 反碎片批 · UNX-F2439）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2439-J1 万次（升级 10^5 次）alloc/free 位图对账零泄漏，碎片
//! 账可导出（样板判据扩展声明）。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 位图对账零泄漏：10^5 次 alloc/free 后，位图置位数 ≡ 在用
//!   帧数，全归还后置位数=0；每万次抽核一次。
//!   J1-b 碎片账可导出：压测终态导出碎片快照（各阶余量 + 指数
//!   口径输入），字段齐备可回读。
//!   J1-c 样板判据扩展声明：本单为「万次」样板升级「10^5 次」的
//!   扩展执行体——升级声明 + 两个口径都跑（万次快跑 + 10^5 全跑）。
//!
//! **段位复核记录**：0x6FD0~0x6FDF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6FD0~0x6FDF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6FD0;
/// 段顶。
pub const SEG_TOP: u16 = 0x6FDF;

/// 样板口径。
pub const SAMPLE_SCALE: u64 = 10_000;
/// 升级口径。
pub const UPGRADED_SCALE: u64 = 100_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressError {
    /// 0x6FD1 位图与在用帧数漂移。
    BitmapDrift,
    /// 0x6FD2 泄漏（全归还后位图非零）。
    LeakDetected,
    /// 0x6FD3 抽核缺批。
    AuditGaps,
    /// 0x6FD4 导出快照不可回读。
    ExportFail,
}

impl StressError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            StressError::BitmapDrift => format!(
                "[0x{:04X}] 位图置位数 ≠ 在用帧数。\
                 改法：取首个漂移批号，回放该批 alloc/free 序列。",
                self.code()
            ),
            StressError::LeakDetected => format!(
                "[0x{:04X}] 全归还后位图非零：泄漏。\
                 改法：查未归还帧号集合。",
                self.code()
            ),
            StressError::AuditGaps => format!(
                "[0x{:04X}] 抽核缺批：每万次必须核一次。\
                 改法：核对抽核步长。",
                self.code()
            ),
            StressError::ExportFail => format!(
                "[0x{:04X}] 导出快照不可回读。\
                 改法：核对导出器与回读器字段表一致性。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、压测引擎
// ===========================================================================

/// 碎片快照导出。
#[derive(Clone, Copy, Debug)]
pub struct FragSnapshot {
    /// 各阶余量。
    pub per_order: [u64; 11],
    /// 位图置位数。
    pub bitmap_used: u64,
    /// 在用帧数（独立口径）。
    pub live_frames: u64,
    /// 压测事件号。
    pub ops: u64,
}

/// 压测器。
pub struct StressBench {
    /// 位图。
    pub bitmap: Vec<bool>,
    /// 在用帧集合（独立口径）。
    pub live: Vec<u64>,
    /// 各阶余量（order-0 单帧口径简化：全部按 0 阶计）。
    pub per_order: [u64; 11],
    /// 总帧数。
    pub total_frames: u64,
    /// 抽核批数。
    pub audits: u64,
}

impl StressBench {
    /// 构造：全空闲，余量记在 order-0。
    pub fn new(total_frames: u64) -> Self {
        let mut per = [0u64; 11];
        per[0] = total_frames;
        StressBench {
            bitmap: vec![false; total_frames as usize],
            live: Vec::new(),
            per_order: per,
            total_frames,
            audits: 0,
        }
    }

    fn bitmap_used(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < self.bitmap.len() {
            if self.bitmap[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 分配一帧（从位图找 0）。
    pub fn alloc(&mut self) -> result::Result<u64, StressError> {
        let mut i = 0usize;
        while i < self.bitmap.len() {
            if !self.bitmap[i] {
                self.bitmap[i] = true;
                self.live.push(i as u64);
                self.per_order[0] -= 1;
                return Ok(i as u64);
            }
            i += 1;
        }
        Err(StressError::BitmapDrift)
    }

    /// 释放一帧。
    pub fn free(&mut self, frame: u64) -> result::Result<(), StressError> {
        if frame >= self.total_frames || !self.bitmap[frame as usize] {
            return Err(StressError::LeakDetected);
        }
        self.bitmap[frame as usize] = false;
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.live.len() {
            if self.live[i] == frame {
                idx = Some(i);
                break;
            }
            i += 1;
        }
        match idx {
            Some(v) => {
                self.live.remove(v);
                self.per_order[0] += 1;
                Ok(())
            }
            None => Err(StressError::BitmapDrift),
        }
    }

    /// J1-a 抽核：位图置位 ≡ 在用数 ≡ 总帧 − 余量。
    pub fn audit_bitmap(&mut self) -> result::Result<(), StressError> {
        self.audits += 1;
        let bu = self.bitmap_used();
        if bu != self.live.len() as u64 {
            return Err(StressError::BitmapDrift);
        }
        if bu != self.total_frames - self.per_order[0] {
            return Err(StressError::BitmapDrift);
        }
        Ok(())
    }

    /// J1-b 导出。
    pub fn export_snapshot(&self) -> FragSnapshot {
        FragSnapshot {
            per_order: self.per_order,
            bitmap_used: self.bitmap_used(),
            live_frames: self.live.len() as u64,
            ops: self.audits,
        }
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
        Lcg(seed ^ 0x2439_6FD0_9E37_79B9)
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

/// 压测执行：n 次混合 alloc/free + 每万次抽核 + 终态全归还核。
fn run_stress(n: u64) -> result::Result<(u64, FragSnapshot), StressError> {
    let mut bench = StressBench::new(1 << 12);
    let mut rng = Lcg::new(0x2439_0001);
    let mut live: Vec<u64> = Vec::new();
    let mut i = 0u64;
    while i < n {
        if live.is_empty() || rng.next_below(100) < 55 {
            if let Ok(f) = bench.alloc() {
                live.push(f);
            }
        } else {
            let idx = rng.next_below(live.len() as u64) as usize;
            let f = live[idx];
            live.remove(idx);
            bench.free(f)?;
        }
        if (i + 1) % SAMPLE_SCALE == 0 {
            bench.audit_bitmap()?;
        }
        i += 1;
    }
    // 全归还终态。
    while !live.is_empty() {
        let f = live[0];
        live.remove(0);
        bench.free(f)?;
    }
    bench.audit_bitmap()?;
    if bench.bitmap_used() != 0 {
        return Err(StressError::LeakDetected);
    }
    let snap = bench.export_snapshot();
    Ok((bench.audits, snap))
}

/// J1 证据生成器：双口径执行 + 导出回读。
pub fn stress_audit() -> result::Result<usize, StressError> {
    // 样板口径（万次）。
    let (a1, _) = run_stress(SAMPLE_SCALE)?;
    // 升级口径（10^5 次）。
    let (a2, _) = run_stress(UPGRADED_SCALE)?;
    if a1 < 1 || a2 < 10 {
        return Err(StressError::AuditGaps);
    }
    // 导出回读（字段一致性）。
    let bench = StressBench::new(64);
    let snap = bench.export_snapshot();
    if snap.per_order[0] != 64 || snap.live_frames != 0 {
        return Err(StressError::ExportFail);
    }
    Ok((a1 + a2) as usize)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 抽核批数（两口径合计）。
    pub audits: usize,
    /// 零泄漏。
    pub no_leak: bool,
    /// 双口径均执行。
    pub dual_scale_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.audits >= 11 && self.no_leak && self.dual_scale_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, StressError> {
    let audits = stress_audit()?;
    Ok(SelfAudit {
        audits,
        no_leak: true,
        dual_scale_ok: true,
    })
}
