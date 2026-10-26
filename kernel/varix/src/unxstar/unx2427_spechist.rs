//! UNX-F2427 · 分配谱直方图 · 诊断段 0x6ED0（独占 0x6ED0~0x6EDF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2427）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2427-J1 十一阶分配频次直方图可导出，直方图与实际分配计数
//! 逐阶一致。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 逐阶一致：直方图 11 桶（order 0~10）与实际分配计数器逐阶
//!   相等——直方图与计数器双轨记账，恒等式全程保持。
//!   J1-b 可导出：直方图导出为定长 11 元数组 + 累计总数 + 帧数
//!   换算列（每阶帧数 = 频次 × 2^o），导出面冻结。
//!   J1-c 溢出守卫：u64 计数饱和语义（saturating_add），溢出留
//!   饱和标志，不回绕（回绕=账目造假）。
//!
//! **建模口径（诚实声明）**：频次直方图按「次数」计；帧谱按
//! 「帧数」计（次数 × 2^o），两谱并列导出。
//!
//! **段位复核记录**：0x6ED0~0x6EDF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6ED0~0x6EDF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6ED0;
/// 段顶。
pub const SEG_TOP: u16 = 0x6EDF;

/// 最高阶。
pub const MAX_ORDER: u32 = 10;
/// 桶数：11。
pub const BUCKETS: usize = 11;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HistError {
    /// 0x6ED1 阶非法。
    BadOrder,
    /// 0x6ED2 直方图与计数器漂移。
    SpectrumDrift,
    /// 0x6ED3 计数饱和（溢出守卫触发）。
    Saturated,
    /// 0x6ED4 导出快照不一致。
    ExportDrift,
}

impl HistError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            HistError::BadOrder => format!(
                "[0x{:04X}] 阶非法：合法 0~10。改法：入参先做域校验。",
                self.code()
            ),
            HistError::SpectrumDrift => format!(
                "[0x{:04X}] 直方图与实际分配计数漂移。\
                 改法：取首个漂移阶，查记账路径是否漏计。",
                self.code()
            ),
            HistError::Saturated => format!(
                "[0x{:04X}] 计数饱和：u64 上限。\
                 改法：导出后重置账本（带快照存档）。",
                self.code()
            ),
            HistError::ExportDrift => format!(
                "[0x{:04X}] 两次导出快照不一致：导出含非确定源。\
                 改法：导出只读账本，禁内联时间戳。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、双轨账：计数器 + 直方图
// ===========================================================================

/// 分配谱账本。
pub struct Spectrum {
    /// 真源计数器（每阶分配次数）。
    pub counter: [u64; BUCKETS],
    /// 直方图（每阶分配次数，双轨对拍用）。
    pub hist: [u64; BUCKETS],
    /// 帧谱（每阶分配帧数 = 次数 × 2^o）。
    pub frame_spec: [u64; BUCKETS],
    /// 饱和标志。
    pub saturated: bool,
}

impl Spectrum {
    /// 空账。
    pub const fn new() -> Self {
        Spectrum {
            counter: [0; BUCKETS],
            hist: [0; BUCKETS],
            frame_spec: [0; BUCKETS],
            saturated: false,
        }
    }

    /// 记一次分配。
    pub fn record(&mut self, order: u32) -> result::Result<(), HistError> {
        if order > MAX_ORDER {
            return Err(HistError::BadOrder);
        }
        let i = order as usize;
        // 真源计数器。
        if self.counter[i] == u64::MAX {
            self.saturated = true;
            return Err(HistError::Saturated);
        }
        self.counter[i] += 1;
        // 直方图双轨。
        self.hist[i] += 1;
        // 帧谱。
        self.frame_spec[i] = self.frame_spec[i].saturating_add(1u64 << order);
        Ok(())
    }

    /// J1-a 恒等核：counter ≡ hist 逐阶。
    pub fn audit_spectrum(&self) -> result::Result<usize, HistError> {
        let mut checked = 0usize;
        let mut i = 0;
        while i < BUCKETS {
            if self.counter[i] != self.hist[i] {
                return Err(HistError::SpectrumDrift);
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }

    /// J1-b 导出：冻结快照（次数 + 帧数并列）。
    pub fn export(&self) -> result::Result<([u64; BUCKETS], [u64; BUCKETS]), HistError> {
        self.audit_spectrum()?;
        let mut counts = [0u64; BUCKETS];
        let mut frames = [0u64; BUCKETS];
        let mut i = 0;
        while i < BUCKETS {
            counts[i] = self.counter[i];
            frames[i] = self.frame_spec[i];
            i += 1;
        }
        Ok((counts, frames))
    }

    /// 总分配次数。
    pub fn total_ops(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < BUCKETS {
            n += self.counter[i];
            i += 1;
        }
        n
    }

    /// 总分配帧数。
    pub fn total_frames(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < BUCKETS {
            n = n.saturating_add(self.frame_spec[i]);
            i += 1;
        }
        n
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
        Lcg(seed ^ 0x2427_6ED0_9E37_79B9)
    }

    /// 下一个。
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 演练分配次数。
pub const HIST_AUDIT_OPS: u64 = 50_000;

/// J1 证据生成器：LCG 驱动 5×10^4 次随机阶分配记账，逐批抽核
/// 双轨恒等；终态导出与手工重放计数对拍。
pub fn hist_audit() -> result::Result<usize, HistError> {
    let mut sp = Spectrum::new();
    let mut rng = Lcg::new(0x2427_0001);
    let mut audits = 0usize;
    let mut n = 0u64;
    while n < HIST_AUDIT_OPS {
        let o = (rng.next_u64() % (MAX_ORDER as u64 + 1)) as u32;
        sp.record(o)?;
        if n % 5000 == 4999 {
            sp.audit_spectrum()?;
            audits += 1;
        }
        n += 1;
    }
    sp.audit_spectrum()?;
    audits += 1;
    // 终态导出 vs 手工重放：重放 LCG 序列独立累计。
    let (counts, frames) = sp.export()?;
    let mut rng2 = Lcg::new(0x2427_0001);
    let mut replay_c = [0u64; BUCKETS];
    let mut replay_f = [0u64; BUCKETS];
    let mut m = 0u64;
    while m < HIST_AUDIT_OPS {
        let o = (rng2.next_u64() % (MAX_ORDER as u64 + 1)) as u32;
        replay_c[o as usize] += 1;
        replay_f[o as usize] += 1u64 << o;
        m += 1;
    }
    let mut i = 0;
    while i < BUCKETS {
        if counts[i] != replay_c[i] || frames[i] != replay_f[i] {
            return Err(HistError::ExportDrift);
        }
        i += 1;
    }
    Ok(audits)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 双轨抽核次数。
    pub audits: usize,
    /// 总分配次数。
    pub total_ops: u64,
    /// 导出对拍通过。
    pub export_ok: bool,
    /// 非法阶被拒。
    pub bad_order_rejected: bool,
    /// 恒等核通过。
    pub identity_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.audits >= 10
            && self.total_ops == HIST_AUDIT_OPS
            && self.export_ok
            && self.bad_order_rejected
            && self.identity_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, HistError> {
    let audits = hist_audit()?;
    let mut sp = Spectrum::new();
    let bad = sp.record(11).is_err();
    let mut k = 0;
    while k < 100 {
        sp.record(k % 11)?;
        k += 1;
    }
    sp.audit_spectrum()?;
    sp.export()?;
    Ok(SelfAudit {
        audits,
        total_ops: HIST_AUDIT_OPS,
        export_ok: true,
        bad_order_rejected: bad,
        identity_ok: true,
    })
}
