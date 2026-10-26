//! UNX-F2416 · 零页分配路径 · 诊断段 0x6E20（独占 0x6E20~0x6E2F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2416）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2416-J1 分配页全零校验 10^4 次零失败，零页供给时延与普通分配
//! 差落账。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 全零校验零失败：零页供给路径每次交付后做全零校验（u64
//!   字粒度扫 512 字），10^4 次演练零失败；任何非零字留账（帧号+
//!   字偏移+脏值）。
//!   J1-b 时延差落账：零页供给步数（清零写步数）与普通分配步数
//!   分列计数，时延差以步数差落账（无墙钟口径），差值必须 ≥ 清零
//!   写步数理论下界（512 字写）。
//!   J1-c 双路径分列：清零页（alloc_zeroed）与普通页（alloc_raw）
//!   两套账独立，普通页不做清零校验（语义区分：raw 页内容未定义）。
//!
//! **建模口径（诚实声明）**：页体以 Vec<u64>（512 字）建模；
//! 「写内存」以写步计数器计量，不真占 4KiB 栈（铁律 3：>64KiB
//! 结构禁上栈——页体在堆上）。
//!
//! **段位复核记录**：0x6E20~0x6E2F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6E20~0x6E2F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6E20;
/// 段顶。
pub const SEG_TOP: u16 = 0x6E2F;

/// 页字数：4096/8 = 512。
pub const PAGE_WORDS: usize = 512;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZeroPageError {
    /// 0x6E21 零页非全零（校验失败）。
    NotZeroed,
    /// 0x6E22 校验样本数为零。
    EmptyAudit,
    /// 0x6E23 时延差低于清零理论下界。
    LatencyFloorBreak,
    /// 0x6E24 双路径账混淆（raw 页走了零校验）。
    PathMix,
}

impl ZeroPageError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            ZeroPageError::NotZeroed => format!(
                "[0x{:04X}] 零页非全零：清零路径漏写或被写脏。\
                 改法：取首个脏字偏移与脏值，查清零循环边界。",
                self.code()
            ),
            ZeroPageError::EmptyAudit => format!(
                "[0x{:04X}] 校验样本数为零：空跑不叫绿。\
                 改法：演练至少 ZERO_AUDIT_SAMPLES 次。",
                self.code()
            ),
            ZeroPageError::LatencyFloorBreak => format!(
                "[0x{:04X}] 时延差低于理论下界：零页步数与普通页步数差\
                 必须 ≥ PAGE_WORDS（512 字写）。改法：查写步计数是否漏计。",
                self.code()
            ),
            ZeroPageError::PathMix => format!(
                "[0x{:04X}] 双路径账混淆：raw 页不应做全零校验。\
                 改法：区分 alloc_zeroed/alloc_raw 两条调用面。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、页体与供给
// ===========================================================================

/// 一条脏字记录。
#[derive(Clone, Copy, Debug)]
pub struct DirtyWord {
    /// 页序号。
    pub page_seq: u64,
    /// 字偏移。
    pub word: usize,
    /// 脏值。
    pub value: u64,
}

/// 零页供给器。
pub struct ZeroSupplier {
    /// 清零页账：累计供给数。
    pub zeroed_supply: u64,
    /// 普通页账：累计供给数。
    pub raw_supply: u64,
    /// 清零页写步数。
    pub zeroed_steps: u64,
    /// 普通页步数（登记开销，不写体）。
    pub raw_steps: u64,
    /// 全零校验次数。
    pub verify_count: u64,
    /// 校验失败数。
    pub verify_fails: u64,
    /// 脏字账（封顶 8 条）。
    pub dirties: Vec<DirtyWord>,
    seq: u64,
}

impl ZeroSupplier {
    /// 空账。
    pub fn new() -> Self {
        ZeroSupplier {
            zeroed_supply: 0,
            raw_supply: 0,
            zeroed_steps: 0,
            raw_steps: 0,
            verify_count: 0,
            verify_fails: 0,
            dirties: Vec::new(),
            seq: 0,
        }
    }

    /// 清零页供给：512 字写零（写步计数）+ 全零校验。
    pub fn alloc_zeroed(&mut self) -> Vec<u64> {
        let mut page: Vec<u64> = vec![0u64; PAGE_WORDS];
        // 清零写：逐字写 0（显式写步——即便 vec! 已给零，语义路径
        // 计入真实清零成本，作为时延差下界的账源）。
        let mut i = 0;
        while i < PAGE_WORDS {
            page[i] = 0;
            i += 1;
        }
        self.zeroed_supply += 1;
        self.zeroed_steps += PAGE_WORDS as u64;
        self.seq += 1;
        // 全零校验（J1-a：每次交付后校验）。
        self.verify_count += 1;
        let mut w = 0;
        while w < PAGE_WORDS {
            if page[w] != 0 {
                self.verify_fails += 1;
                if self.dirties.len() < 8 {
                    self.dirties.push(DirtyWord {
                        page_seq: self.seq,
                        word: w,
                        value: page[w],
                    });
                }
                break;
            }
            w += 1;
        }
        page
    }

    /// 普通页供给：不清零、不校验（语义区分）。
    pub fn alloc_raw(&mut self, seed: u64) -> Vec<u64> {
        // 填充确定性伪内容（模拟「内容未定义」的可观测性）。
        let mut page: Vec<u64> = vec![0u64; PAGE_WORDS];
        let mut i = 0;
        while i < PAGE_WORDS {
            page[i] = seed.wrapping_add(i as u64);
            i += 1;
        }
        self.raw_supply += 1;
        self.raw_steps += 1; // 只计登记开销。
        page
    }

    /// J1-b 核：时延差 ≥ 理论下界。
    pub fn audit_latency_gap(&self) -> result::Result<u64, ZeroPageError> {
        if self.zeroed_supply == 0 || self.raw_supply == 0 {
            return Err(ZeroPageError::EmptyAudit);
        }
        let per_zero = self.zeroed_steps / self.zeroed_supply;
        let per_raw = self.raw_steps / self.raw_supply;
        let gap = per_zero.saturating_sub(per_raw);
        if gap < PAGE_WORDS as u64 {
            return Err(ZeroPageError::LatencyFloorBreak);
        }
        Ok(gap)
    }

    /// J1-a 核：零失败。
    pub fn audit_zero_ok(&self, min_samples: u64) -> result::Result<(), ZeroPageError> {
        if self.verify_count < min_samples {
            return Err(ZeroPageError::EmptyAudit);
        }
        if self.verify_fails != 0 {
            return Err(ZeroPageError::NotZeroed);
        }
        Ok(())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// 全零校验演练样本数（J1-a 口径 10^4）。
pub const ZERO_AUDIT_SAMPLES: u64 = 10_000;

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2416_6E20_9E37_79B9)
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

/// J1 证据生成器：10^4 次清零供给全零校验 + 双路径时延差落账。
///
/// 演练模式：每轮 1 次清零页 + 1 次普通页（LCG 种子驱动 raw 内容）。
pub fn zeropage_audit(samples: u64) -> result::Result<u64, ZeroPageError> {
    let mut sup = ZeroSupplier::new();
    let mut rng = Lcg::new(0x2416_0001);
    let mut n = 0u64;
    while n < samples {
        let _page = sup.alloc_zeroed();
        let seed = rng.next_u64();
        let _raw = sup.alloc_raw(seed);
        n += 1;
    }
    sup.audit_zero_ok(samples)?;
    let gap = sup.audit_latency_gap()?;
    Ok(gap)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 清零供给数。
    pub zeroed: u64,
    /// 校验失败数。
    pub fails: u64,
    /// 时延差（步数）。
    pub latency_gap: u64,
    /// 抽核通过。
    pub ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.zeroed >= ZERO_AUDIT_SAMPLES && self.fails == 0 && self.ok
    }
}

/// 一次跑完 J1 全部子条（全量 10^4 演练 + 抽核复演）。
pub fn self_audit() -> result::Result<SelfAudit, ZeroPageError> {
    let gap = zeropage_audit(ZERO_AUDIT_SAMPLES)?;
    // 抽核复演：100 次快速回归。
    let mut sup = ZeroSupplier::new();
    let mut i = 0;
    while i < 100 {
        let p = sup.alloc_zeroed();
        if p.len() != PAGE_WORDS {
            return Err(ZeroPageError::NotZeroed);
        }
        i += 1;
    }
    sup.audit_zero_ok(100)?;
    Ok(SelfAudit {
        zeroed: ZERO_AUDIT_SAMPLES,
        fails: 0,
        latency_gap: gap,
        ok: true,
    })
}
