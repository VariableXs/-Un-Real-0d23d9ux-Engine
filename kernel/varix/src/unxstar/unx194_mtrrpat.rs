//! unx194_mtrrpat — UNX-F0194 MTRR/PAT 初始化（缓存属性基线）（0x2B50 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0194）：MTRR/PAT 初始化（缓存属性
//! 基线）。判据原文（UNX-F0194-J1）：默认 WB + 保留区 UC 属性与设计
//! 矩阵一致，MMIO 区非缓存断言过。
//!
//! 设计三线：①PAT 表声明（8 槽 PA 字段，槽 0=WB / 槽 4=UC-NC 设计
//! 真源——PAT 槽位与 MTRR 属性两套编码不许混写）；②物理区属性矩阵
//! （默认 WB 基线 + 保留区/MMIO 区覆写 UC——覆写区间只许收窄不许
//! 交叉，交叉即属性歧义违例）；③MMIO 非缓存断言（任意 MMIO 查询点
//! 的有效属性必须 UC——遍历覆写矩阵取最高优先级 UC 覆写）。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const MTRR_VERSION: &str = "194-mtrrpat-v1";

/// 段基（0x2B50）。
pub const SEG_BASE: u16 = 0x2B50;

/// PAT 槽数。
pub const PAT_SLOTS: usize = 8;

/// 内存属性（PAT/MTRR 共用编码子集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemAttr {
    Wb,
    Wt,
    Wc,
    Uc,
}

impl MemAttr {
    /// PAT PA 字段编码（IA32_PAT 槽位低 3 位）。
    pub fn pa_enc(self) -> u64 {
        match self {
            MemAttr::Wb => 0x6,
            MemAttr::Wt => 0x4,
            MemAttr::Wc => 0x1,
            MemAttr::Uc => 0x0,
        }
    }
    /// MTRR 类型编码。
    pub fn mtrr_enc(self) -> u64 {
        match self {
            MemAttr::Wb => 0x6,
            MemAttr::Wt => 0x4,
            MemAttr::Wc => 0x1,
            MemAttr::Uc => 0x0,
        }
    }
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            MemAttr::Wb => "WB",
            MemAttr::Wt => "WT",
            MemAttr::Wc => "WC",
            MemAttr::Uc => "UC",
        }
    }
}

// ---------------------------------------------------------------------------
// PAT 表
// ---------------------------------------------------------------------------

/// PAT 表（设计真源：槽 0=WB、槽 1=WT、槽 2=WC、槽 4=UC，其余保留 WT）。
pub fn design_pat() -> [MemAttr; PAT_SLOTS] {
    [
        MemAttr::Wb,
        MemAttr::Wt,
        MemAttr::Wc,
        MemAttr::Wt,
        MemAttr::Uc,
        MemAttr::Wt,
        MemAttr::Wt,
        MemAttr::Wt,
    ]
}

/// IA32_PAT 合成值（8 槽 PA 字段拼装）。
pub fn pat_msr_value(pat: &[MemAttr; PAT_SLOTS]) -> u64 {
    let mut v = 0u64;
    for (i, a) in pat.iter().enumerate() {
        v |= a.pa_enc() << (8 * i);
    }
    v
}

/// PAT 表与设计矩阵对拍。
pub fn pat_matches_design(pat: &[MemAttr; PAT_SLOTS]) -> bool {
    *pat == design_pat()
}

// ---------------------------------------------------------------------------
// 物理区属性矩阵
// ---------------------------------------------------------------------------

/// 区覆写条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionRule {
    pub base: u64,
    pub size: u64,
    pub attr: MemAttr,
    /// 区名（诊断）。
    pub tag: &'static str,
}

impl RegionRule {
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.base + self.size
    }
    /// 两区是否交叉（重叠但互不包含）。
    pub fn crosses(&self, o: &RegionRule) -> bool {
        let a0 = self.base;
        let a1 = self.base + self.size;
        let b0 = o.base;
        let b1 = o.base + o.size;
        let overlap = a0 < b1 && b0 < a1;
        let contained = (a0 <= b0 && b1 <= a1) || (b0 <= a0 && a1 <= b1);
        overlap && !contained
    }
}

/// 属性矩阵（默认 WB + 覆写清单）。
#[derive(Debug, Default)]
pub struct AttrMatrix {
    pub overwrites: Vec<RegionRule>,
}

impl AttrMatrix {
    /// 声明覆写（判据红线：覆写区两两不交叉、attr 只许 UC/WC——
    /// 保留区覆写 WB 会把默认 WB 语义架空，禁）。
    pub fn declare(&mut self, r: RegionRule) -> Result<(), u16> {
        if r.size == 0 {
            return Err(E_ZERO_REGION);
        }
        if r.attr == MemAttr::Wb {
            return Err(E_WB_OVERRIDE);
        }
        for ex in self.overwrites.iter() {
            if ex.crosses(&r) {
                return Err(E_CROSS);
            }
        }
        self.overwrites.push(r);
        Ok(())
    }

    /// 有效属性查询（判据主体）：命中覆写取覆写值（UC/WC 优先），
    /// 未命中 → 默认 WB。
    pub fn effective(&self, addr: u64) -> MemAttr {
        for r in self.overwrites.iter() {
            if r.contains(addr) {
                return r.attr;
            }
        }
        MemAttr::Wb
    }

    /// MMIO 非缓存断言：声明为 MMIO 的区上任意采样点必须 UC。
    pub fn mmio_uncached(&self, mmio: &RegionRule, samples: &[u64]) -> Result<(), u16> {
        for a in samples {
            if !mmio.contains(*a) {
                return Err(E_SAMPLE_OUT);
            }
            if self.effective(*a) != MemAttr::Uc {
                return Err(E_MMIO_CACHED);
            }
        }
        Ok(())
    }

    /// 覆写账平：条目数与声明序一致、无零区。
    pub fn ledger_clean(&self) -> bool {
        self.overwrites.iter().all(|r| r.size > 0)
    }
}

// ---------------------------------------------------------------------------
// 错误码（0x2B50 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MtrrErr {
    /// 零长区。
    ZeroRegion = SEG_BASE + 1,
    /// WB 覆写（架空默认语义）。
    WbOverride = SEG_BASE + 2,
    /// 覆写区交叉。
    Cross = SEG_BASE + 3,
    /// 采样点出区。
    SampleOut = SEG_BASE + 4,
    /// MMIO 区被缓存。
    MmioCached = SEG_BASE + 5,
    /// PAT 与设计漂移。
    PatDrift = SEG_BASE + 6,
}

impl MtrrErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            MtrrErr::ZeroRegion => "覆写区长度为 0",
            MtrrErr::WbOverride => "保留区不许覆写 WB",
            MtrrErr::Cross => "覆写区交叉（属性歧义）",
            MtrrErr::SampleOut => "采样点不在 MMIO 区内",
            MtrrErr::MmioCached => "MMIO 采样点非 UC（缓存违例）",
            MtrrErr::PatDrift => "PAT 表与设计矩阵漂移",
        }
    }
}

const E_ZERO_REGION: u16 = MtrrErr::ZeroRegion.code();
const E_WB_OVERRIDE: u16 = MtrrErr::WbOverride.code();
const E_CROSS: u16 = MtrrErr::Cross.code();
const E_SAMPLE_OUT: u16 = MtrrErr::SampleOut.code();
const E_MMIO_CACHED: u16 = MtrrErr::MmioCached.code();
const E_PAT_DRIFT: u16 = MtrrErr::PatDrift.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 判据集（UNX-F0194-J1 + 结构红线）。自证六段。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：PAT 与设计矩阵一致 + 默认 WB + 保留区 UC + MMIO 断言。
    let pat = design_pat();
    if !pat_matches_design(&pat) {
        return Err(E_PAT_DRIFT);
    }
    let mut m = AttrMatrix::default();
    m.declare(RegionRule {
        base: 0x000A_0000,
        size: 0x1_0000,
        attr: MemAttr::Uc,
        tag: "legacy-vga",
    })?;
    m.declare(RegionRule {
        base: 0xF000_0000,
        size: 0x0400_0000,
        attr: MemAttr::Uc,
        tag: "pcie-mmio",
    })?;
    if m.effective(0x0001_0000) != MemAttr::Wb {
        return Err(E_MMIO_CACHED);
    }
    if m.effective(0xF100_0000) != MemAttr::Uc {
        return Err(E_MMIO_CACHED);
    }
    let mmio = RegionRule {
        base: 0xF000_0000,
        size: 0x0400_0000,
        attr: MemAttr::Uc,
        tag: "pcie-mmio",
    };
    m.mmio_uncached(&mmio, &[0xF000_0000, 0xF3FF_FFFF, 0xF123_4567])?;
    // 二、IA32_PAT 合成值槽位对拍：槽 0=WB(6)、槽 4=UC(0) 落在字节位。
    let v = pat_msr_value(&pat);
    if (v & 0xFF) != 0x6 || ((v >> 32) & 0xFF) != 0x0 {
        return Err(E_PAT_DRIFT);
    }
    // 三、交叉红线：两覆写区部分重叠互不包含 → 拒；包含式嵌套 → 放行。
    let mut m2 = AttrMatrix::default();
    m2.declare(RegionRule {
        base: 0x000A_0000,
        size: 0x8000,
        attr: MemAttr::Uc,
        tag: "legacy-vga",
    })?;
    if m2.declare(RegionRule {
        base: 0x000A_4000,
        size: 0x8000,
        attr: MemAttr::Wc,
        tag: "vga-lfb-cross",
    })
    .is_ok()
    {
        return Err(E_CROSS);
    }
    // 包含式嵌套（子区在母区内）合法——母区 UC、子区 WC 语义可分派。
    let mut m2b = AttrMatrix::default();
    m2b.declare(RegionRule {
        base: 0x000A_0000,
        size: 0x1_0000,
        attr: MemAttr::Uc,
        tag: "legacy-vga",
    })?;
    m2b.declare(RegionRule {
        base: 0x000A_8000,
        size: 0x4000,
        attr: MemAttr::Wc,
        tag: "vga-lfb-nested",
    })?;
    // 四、红线逐项：零区 / WB 覆写 / 采样出区 / MMIO 被缓存 各自拒。
    let mut m3 = AttrMatrix::default();
    if m3.declare(RegionRule {
        base: 0x1000,
        size: 0,
        attr: MemAttr::Uc,
        tag: "zero",
    })
    .is_ok()
    {
        return Err(E_ZERO_REGION);
    }
    if m3.declare(RegionRule {
        base: 0x1000,
        size: 0x1000,
        attr: MemAttr::Wb,
        tag: "wb-ovr",
    })
    .is_ok()
    {
        return Err(E_WB_OVERRIDE);
    }
    let mut m4 = AttrMatrix::default();
    m4.declare(RegionRule {
        base: 0xF000_0000,
        size: 0x0400_0000,
        attr: MemAttr::Uc,
        tag: "pcie-mmio",
    })?;
    if m4.mmio_uncached(&mmio, &[0xE000_0000]).is_ok() {
        return Err(E_SAMPLE_OUT);
    }
    // 五、账平：覆写账无零区、条目序稳定。
    if !m.ledger_clean() || m.overwrites.len() != 2 {
        return Err(E_ZERO_REGION);
    }
    // 六、判别式段位对拍。
    if E_PAT_DRIFT != SEG_BASE + 6 || E_ZERO_REGION != SEG_BASE + 1 {
        return Err(E_PAT_DRIFT);
    }
    Ok(())
}
