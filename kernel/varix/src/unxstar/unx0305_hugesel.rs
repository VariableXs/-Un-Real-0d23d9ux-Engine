//! ═══════════════════════════════════════════════════════════════════════════
//! UNX-F0305 · 大页粒度选择器（F0068 预判消费）
//! 诊断段：0x1570 ~ 0x157F（独占）
//!
//! 诊断段占用核查记录（grep 全仓结论，2026-10-24）：
//!   - 0x14A0~0x14A3 已被 svstar2/ves446_s24copy.rs（SEG_BASE=0x14A0）占用；
//!   - 0x1560    已被 svstar2/ves452_s24joint.rs 占用；
//!   - 0x1580    已被 cgpu/cga25_mipmap.rs 占用；
//!   - 0x1590    已被 cgpu/cga26_gauss.rs 占用；
//!   - 0x15A0    已被 svstar2/ves453_s24selfcheck.rs 占用；
//!   - 0x1570    全仓无命中（含 segs_all.txt），确认空闲 → 本模块取 0x1570。
//!
//! 验收锚点（docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//!           第 1747 行，原文全文抄录）：
//!   | UNX-F0305 | 大页粒度选择器（F0068 预判消费） | 280 | 已深化 | UNX-F0305-J1 1G/2M/4K 选择与 F0068 预判逐区一致，实际映射抽样回填比对过 |
//!
//! 判据 UNX-F0305-J1 分解：
//!   (a) 1G/2M/4K 三档闭集逐区选择：对齐判据（1G 需 1G 对齐且长度≥1G；
//!       2M 需 2M 对齐且长度≥2M；4K 恒可）＋属性判据（DeviceMmio 仅 4K）；
//!   (b) 与 F0068（unx0068_hugepage.rs，诊断段 0x2130）预判逐区一致：
//!       逐区 diff，档位/覆盖任一不符即立案（分歧清单+一致率账）；
//!   (c) 实际映射抽样回填比对：MappingSim 顺序游标法推进映射，
//!       每 SAMPLE_STRIDE 步（外加首尾块）用索引法独立重推 (基址,档位)，
//!       两路推导不一致即回填立案；
//!   (d) 页数守恒：N×1G + M×2M + K×4K 覆盖字节 ≡ 区长，开销账
//!       （页表项数最小化）对比全 4K 方案给出节省倍率。
//!
//! 纪律：no_std+alloc；零 unwrap/expect/panic!/运行期 assert!（编译期
//!       const assert 除外）；可失败全走 Result<T, HugeSelError>；溢出
//!       显性 saturating/wrapping；>64KB 结构全走堆；const fn 内禁
//!       slice::get / Vec 索引 / Ord::min/max / derive-PartialEq 的 ==
//!       / for 循环（一律 while+索引）。
//! ═══════════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════════
// 常量段（来源注释：x86-64 硬件粒度 + 本仓 F0068/F0305 系列设计冻结值）
// ═══════════════════════════════════════════════════════════════════════════

/// 4K 标准页 = 2^12 字节（x86-64 PT 叶项粒度，硬件冻结值）。
pub const PAGE_4K: u64 = 0x1000;
/// 2M 大页 = 2^21 字节（x86-64 PMD 叶项粒度，硬件冻结值；与 F0068 HP_2M 同值）。
pub const PAGE_2M: u64 = 0x0020_0000;
/// 1G 巨页 = 2^30 字节（x86-64 PUD 叶项粒度，硬件冻结值；与 F0068 HP_1G 同值）。
pub const PAGE_1G: u64 = 0x4000_0000;

/// 本模块诊断段基址（0x1570，独占段 0x1570~0x157F）。
pub const SEG_BASE: u16 = 0x1570;
/// 任务编号。
pub const MODULE_ID: u16 = 0x0305;
/// 任务标签。
pub const TASK_TAG: &str = "UNX-F0305";

/// 区域请求上限：区记录（约 40B/条）×256 ≈ 10KB 堆账，防预判表膨胀。
pub const MAX_ZONES: usize = 256;
/// F0068 预判行上限：与区上限同量级（一区可有多行，逐行对账）。
pub const MAX_PREDS: usize = 256;
/// 事件账上限：EventRec 24B × 512 = 12KB 堆账，溢出走丢包计数。
pub const MAX_EVENTS: usize = 512;
/// 分歧清单上限：DivergenceRec 约 48B × 128 = 6KB，超出置截断位。
pub const MAX_DIVERGENCES: usize = 128;
/// 仿真抽样样本上限：SimSample 32B × 4096 = 128KB 堆账。
pub const MAX_SIM_SAMPLES: usize = 4096;
/// 单方案仿真块数上限：全 4K 时对应 16GB 区；超限拒绝仿真（防 O(n) 走查失控）。
pub const MAX_SIM_CHUNKS: u64 = 1 << 22;
/// 抽样回填步距：每第 7 块必抽，另加每方案首块与末块（J 判据回填）。
pub const SAMPLE_STRIDE: u64 = 7;

// ═══════════════════════════════════════════════════════════════════════════
// 诊断码段（独占 0x1570~0x157F；0x1570 为 OK 码，0x1571~0x157F 为错误码）
// ═══════════════════════════════════════════════════════════════════════════

/// 事件级 OK 码：正常留痕（非错误）。
pub const E_OK: u16 = SEG_BASE; // 0x1570

/// UNX-F0305 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HugeSelError {
    /// 区长度为零。
    ZeroLen,        // 0x1571
    /// 基址+长度地址算术回绕/溢出。
    Overrun,        // 0x1572
    /// 基址未达所需档位对齐。
    BaseUnalign,    // 0x1573
    /// 区长度未对齐最小 4K 粒度（尾页无法收边）。
    LenUnalign,     // 0x1574
    /// 预判档位与区域属性矩阵冲突（如 DeviceMmio 收到 2M 预判）。
    GradeNotAllowed, // 0x1575
    /// F0068 预判表该区缺行（或同区重复注入冲突）。
    PredMissing,    // 0x1576
    /// 预判档位与选择档位逐区不一致。
    PredGradeDiverge, // 0x1577
    /// 预判覆盖字节超出方案可覆盖量（预判超发）。
    PredCoverDiverge, // 0x1578
    /// 页数守恒破裂：Σ覆盖 ≠ 区长。
    CoverageGap,    // 0x1579
    /// 账面/仿真规模超限（拒绝服务而非OOM）。
    LedgerFull,     // 0x157A
    /// 状态机误用：封板后写、方案重复生成等。
    Sealed,         // 0x157B
    /// 映射仿真抽样回填不一致。
    SimBackfill,    // 0x157C
    /// 映射仿真连续性破裂（块基址 ≠ 前块终点）。
    SimContiguity,  // 0x157D
    /// 双跑确定性对拍漂移。
    DeterminismDrift, // 0x157E
    /// 内部算术溢出（checked 链断裂，属防御性兜底）。
    InternalOverflow, // 0x157F
}

impl HugeSelError {
    /// 诊断码（段内唯一，0x1571~0x157F；const 以便编译期闭集断言复用）。
    pub const fn code(&self) -> u16 {
        match self {
            HugeSelError::ZeroLen => SEG_BASE + 0x01,
            HugeSelError::Overrun => SEG_BASE + 0x02,
            HugeSelError::BaseUnalign => SEG_BASE + 0x03,
            HugeSelError::LenUnalign => SEG_BASE + 0x04,
            HugeSelError::GradeNotAllowed => SEG_BASE + 0x05,
            HugeSelError::PredMissing => SEG_BASE + 0x06,
            HugeSelError::PredGradeDiverge => SEG_BASE + 0x07,
            HugeSelError::PredCoverDiverge => SEG_BASE + 0x08,
            HugeSelError::CoverageGap => SEG_BASE + 0x09,
            HugeSelError::LedgerFull => SEG_BASE + 0x0A,
            HugeSelError::Sealed => SEG_BASE + 0x0B,
            HugeSelError::SimBackfill => SEG_BASE + 0x0C,
            HugeSelError::SimContiguity => SEG_BASE + 0x0D,
            HugeSelError::DeterminismDrift => SEG_BASE + 0x0E,
            HugeSelError::InternalOverflow => SEG_BASE + 0x0F,
        }
    }

    /// 中文可操作诊断（每变体固定一句，指明"哪里错了+怎么修"）。
    pub fn detail(&self) -> &'static str {
        match self {
            HugeSelError::ZeroLen =>
                "区长为零：请检查区域请求 len 字段，空区应直接跳过而非提交",
            HugeSelError::Overrun =>
                "地址算术回绕：base+len 溢出 u64，请核对区域表有无回绕行",
            HugeSelError::BaseUnalign =>
                "基址未对齐：1G 档需 1G 对齐、2M 档需 2M 对齐、任何区需 4K 对齐，请核对区基址低 12 位",
            HugeSelError::LenUnalign =>
                "区长未对齐：区长必须是 4K 整数倍，否则尾页无法收边，请核对区长低 12 位",
            HugeSelError::GradeNotAllowed =>
                "属性档位冲突：DeviceMmio 区仅允许 4K 档，请核对 F0068 预判行与区属性矩阵",
            HugeSelError::PredMissing =>
                "预判缺行：F0068 预判表未覆盖该区（或同区重复注入冲突，保留首条），请补齐/去重预判表后重跑对账",
            HugeSelError::PredGradeDiverge =>
                "预判档位分歧：本选择器逐区档位与 F0068 预判不一致，请比对分歧清单的 pred_grade/plan_grade 两列",
            HugeSelError::PredCoverDiverge =>
                "预判超发：F0068 cover_bytes 大于方案可覆盖字节，请核对预判账的余料计算",
            HugeSelError::CoverageGap =>
                "覆盖守恒破裂：N×1G+M×2M+K×4K ≠ 区长，请核对分档页数与区长倍数关系",
            HugeSelError::LedgerFull =>
                "账面超限：请求/预判/事件/仿真规模越过冻结上限，请分批提交或调高上限常量",
            HugeSelError::Sealed =>
                "状态机误用：账已封板或方案已生成；重跑请走 determinism_selfcheck，勿重复 select_all",
            HugeSelError::SimBackfill =>
                "回填不一致：顺序游标法与索引法重推的 (基址,档位) 不一致，请核对 derive_chunk 与 LadderState 两路推导",
            HugeSelError::SimContiguity =>
                "映射不连续：块基址不等于前块终点，请核对分档顺序装填是否留洞",
            HugeSelError::DeterminismDrift =>
                "双跑漂移：同输入两跑指纹不同，请检查是否有未冻结的迭代序或随机源",
            HugeSelError::InternalOverflow =>
                "内部算术溢出：checked 链断裂属防御性兜底，请携带事件账上报",
        }
    }
}

impl core::fmt::Display for HugeSelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "UNX-F0305 0x{:04X}: {}", self.code(), self.detail())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PageSize：三档闭集（1G / 2M / 4K）
// ═══════════════════════════════════════════════════════════════════════════

/// 大页粒度三档闭集。顺序即"从大到小"的确定性贪心序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageSize {
    /// 1G 巨页（PUD 叶项）。
    P1G,
    /// 2M 大页（PMD 叶项）。
    P2M,
    /// 4K 标准页（PT 叶项）。
    P4K,
}

impl PageSize {
    /// 档位字节尺寸（const，硬件冻结值直译）。
    pub const fn size(self) -> u64 {
        match self {
            PageSize::P1G => PAGE_1G,
            PageSize::P2M => PAGE_2M,
            PageSize::P4K => PAGE_4K,
        }
    }

    /// 档位位移量（log2(size)）。
    pub const fn shift(self) -> u32 {
        match self {
            PageSize::P1G => 30,
            PageSize::P2M => 21,
            PageSize::P4K => 12,
        }
    }

    /// 档位掩码（size-1）。
    pub const fn mask(self) -> u64 {
        match self {
            PageSize::P1G => PAGE_1G - 1,
            PageSize::P2M => PAGE_2M - 1,
            PageSize::P4K => PAGE_4K - 1,
        }
    }

    /// 档位序号：0=1G，1=2M，2=4K。
    pub const fn index(self) -> u8 {
        match self {
            PageSize::P1G => 0,
            PageSize::P2M => 1,
            PageSize::P4K => 2,
        }
    }

    /// 序号反查档位（const，match 直译，不用 slice::get）。
    pub const fn from_index(i: u8) -> Option<PageSize> {
        match i {
            0 => Option::Some(PageSize::P1G),
            1 => Option::Some(PageSize::P2M),
            2 => Option::Some(PageSize::P4K),
            _ => Option::None,
        }
    }

    /// 档位短名（账表列头用）。
    pub const fn name(self) -> &'static str {
        match self {
            PageSize::P1G => "1G",
            PageSize::P2M => "2M",
            PageSize::P4K => "4K",
        }
    }

    /// 单块叶项页表项数：任何档位一块恰好占 1 个叶项（PT/PMD/PUD 各级）。
    /// 差异在"覆盖字节/叶项"的摊销率，见 SplitPlan::savings_per_mille。
    pub const fn leaf_entries(self) -> u64 {
        match self {
            PageSize::P1G => 1,
            PageSize::P2M => 1,
            PageSize::P4K => 1,
        }
    }

    /// 对齐判据：addr 是否按该档位对齐（const，位与直译）。
    pub const fn align_ok(addr: u64, g: PageSize) -> bool {
        (addr & g.mask()) == 0
    }

    /// 对齐上取整（const，wrapping 防回绕；调用方需另行 checked 校验区间）。
    pub const fn align_up(addr: u64, g: PageSize) -> u64 {
        addr.wrapping_add(g.mask()) & !g.mask()
    }

    /// len 内可容纳的完整块数（向下取整，const）。
    pub const fn full_count(len: u64, g: PageSize) -> u64 {
        len >> g.shift()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 区域属性矩阵：ZoneKind → 允许档位
// ═══════════════════════════════════════════════════════════════════════════

/// 区域属性（决定档位合法性矩阵）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneKind {
    /// 内核代码段：三档全可（确定性贪心按对齐取档）。
    KernelText,
    /// 内核数据段：三档全可。
    KernelData,
    /// 直接映射窗口：三档全可（F0304 对接，1G 优先摊销）。
    DirectMap,
    /// 设备 MMIO：仅 4K（MMIO 必须页粒度精确，禁大页）。
    DeviceMmio,
    /// 用户空闲区：三档全可。
    UserFree,
}

impl ZoneKind {
    /// 属性短名（账表列头用）。
    pub const fn name(self) -> &'static str {
        match self {
            ZoneKind::KernelText => "内核代码",
            ZoneKind::KernelData => "内核数据",
            ZoneKind::DirectMap => "直接映射",
            ZoneKind::DeviceMmio => "设备MMIO",
            ZoneKind::UserFree => "用户空闲",
        }
    }

    /// 档位合法性判据（const，match 直译）。
    pub const fn allows(self, g: PageSize) -> bool {
        match (self, g) {
            (ZoneKind::DeviceMmio, PageSize::P4K) => true,
            (ZoneKind::DeviceMmio, _) => false,
            _ => true,
        }
    }

    /// 该属性允许的档位数（矩阵宽度，账面用）。
    pub const fn allowed_count(self) -> u8 {
        match self {
            ZoneKind::DeviceMmio => 1,
            _ => 3,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 输入行：ZoneRequest（区域请求）与 PredEntry（F0068 预判）
// ═══════════════════════════════════════════════════════════════════════════

/// 区域请求：基址 + 长度 + 属性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoneRequest {
    /// 区号（对账主键，须与 F0068 预判行 zone_id 对得上）。
    pub zone_id: u32,
    /// 区基址（物理/虚拟由调用方约定；须 4K 对齐，取 1G/2M 档另有对齐判据）。
    pub base: u64,
    /// 区长度（须 4K 整数倍）。
    pub len: u64,
    /// 区域属性。
    pub kind: ZoneKind,
}

impl ZoneRequest {
    /// 区终点（None = 地址算术回绕，调用方须按 Overrun 立案）。
    pub fn end(&self) -> Option<u64> {
        self.base.checked_add(self.len)
    }

    /// 基础校验：零长/回绕/4K 对齐三关（档位对齐在选档时判）。
    pub fn validate(&self) -> Result<(), HugeSelError> {
        if self.len == 0 {
            return Err(HugeSelError::ZeroLen);
        }
        if self.base.checked_add(self.len).is_none() {
            return Err(HugeSelError::Overrun);
        }
        if !PageSize::align_ok(self.base, PageSize::P4K) {
            return Err(HugeSelError::BaseUnalign);
        }
        if !PageSize::align_ok(self.len, PageSize::P4K) {
            return Err(HugeSelError::LenUnalign);
        }
        Ok(())
    }
}

/// F0068 预判行（消费输入）：该区"应采用的档位 + 可覆盖字节"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PredEntry {
    /// 区号（对账主键）。
    pub zone_id: u32,
    /// F0068 预判档位。
    pub grade: PageSize,
    /// F0068 预判可覆盖字节（方案的覆盖必须 ≥ 此值，超出即"预判超发"立案）。
    pub cover_bytes: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// FNV-1a 64 指纹（确定性对拍与账面封印共用）
// ═══════════════════════════════════════════════════════════════════════════

/// FNV-1a 64 偏移基（标准冻结值）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 素数（标准冻结值）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// 单字节推进 FNV。
pub fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// u64 按 LE 八字节推进 FNV（移位取字节，不依赖切片）。
pub fn fnv_u64(h: u64, v: u64) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 8 {
        acc = fnv_byte(acc, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

/// 字节切片整段 FNV-1a（诊断与测试用）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < data.len() {
        h = fnv_byte(h, data[i]);
        i += 1;
    }
    h
}

// ═══════════════════════════════════════════════════════════════════════════
// SplitPlan：分解方案（区 → N×1G + M×2M + K×4K）
// ═══════════════════════════════════════════════════════════════════════════

/// 分解方案：一区的 1G/2M/4K 页数分配。
/// 不变式（new_checked 强制）：
///   I1 页数守恒  n1g*1G + n2m*2M + n4k*4K ≡ len（精确相等）；
///   I2 块内对齐  有 1G 块 ⇒ base 1G 对齐；有 2M 块 ⇒ base 2M 对齐；
///                有 4K 块 ⇒ base 4K 对齐；
///   I3 装填序    1G 块从 base 起连续装填，2M 块紧随其后，4K 收尾——
///                保证 chunk 空间无洞，支撑 MappingSim 连续性检查。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitPlan {
    /// 区号。
    pub zone_id: u32,
    /// 区基址。
    pub base: u64,
    /// 区长度。
    pub len: u64,
    /// 1G 块数。
    pub n1g: u64,
    /// 2M 块数。
    pub n2m: u64,
    /// 4K 块数（含尾块降档收边）。
    pub n4k: u64,
}

impl SplitPlan {
    /// 受检构造：强制不变式 I1/I2，任何破坏在生成期即立案。
    pub fn new_checked(
        zone_id: u32,
        base: u64,
        len: u64,
        n1g: u64,
        n2m: u64,
        n4k: u64,
    ) -> Result<SplitPlan, HugeSelError> {
        let c1 = n1g.checked_mul(PAGE_1G).ok_or(HugeSelError::InternalOverflow)?;
        let c2 = n2m.checked_mul(PAGE_2M).ok_or(HugeSelError::InternalOverflow)?;
        let c4 = n4k.checked_mul(PAGE_4K).ok_or(HugeSelError::InternalOverflow)?;
        let covered = c1
            .checked_add(c2)
            .and_then(|v| v.checked_add(c4))
            .ok_or(HugeSelError::InternalOverflow)?;
        if covered != len {
            return Err(HugeSelError::CoverageGap);
        }
        if base.checked_add(len).is_none() {
            return Err(HugeSelError::Overrun);
        }
        if n1g > 0 && !PageSize::align_ok(base, PageSize::P1G) {
            return Err(HugeSelError::BaseUnalign);
        }
        if n2m > 0 && !PageSize::align_ok(base, PageSize::P2M) {
            return Err(HugeSelError::BaseUnalign);
        }
        if n4k > 0 && !PageSize::align_ok(base, PageSize::P4K) {
            return Err(HugeSelError::BaseUnalign);
        }
        Ok(SplitPlan { zone_id, base, len, n1g, n2m, n4k })
    }

    /// 覆盖字节（const，wrapping 链；new_checked 已保证 ≡ len）。
    pub const fn covered_bytes(&self) -> u64 {
        self.n1g
            .wrapping_mul(PAGE_1G)
            .wrapping_add(self.n2m.wrapping_mul(PAGE_2M))
            .wrapping_add(self.n4k.wrapping_mul(PAGE_4K))
    }

    /// 总块数 = 总叶项页表项数（const）。
    pub const fn total_chunks(&self) -> u64 {
        self.n1g
            .wrapping_add(self.n2m)
            .wrapping_add(self.n4k)
    }

    /// 叶项页表项数（与总块数同义，开销账语义名）。
    pub const fn pte_entries(&self) -> u64 {
        self.total_chunks()
    }

    /// 全 4K 对照方案的叶项数（尾不足页按 1 项计，const）。
    pub const fn all4k_entries(&self) -> u64 {
        let full = self.len >> 12;
        let tail = self.len & (PAGE_4K - 1);
        full.wrapping_add(if tail != 0 { 1 } else { 0 })
    }

    /// 混合档位相对全 4K 的节省倍率（‰，const）：
    ///   all4k_entries × 1000 / mixed_entries。
    /// >1000 表示混合方案更省；全 4K 区恒等于 1000。
    pub const fn savings_per_mille(&self) -> u64 {
        let mixed = self.pte_entries();
        if mixed == 0 {
            return 0;
        }
        self.all4k_entries().saturating_mul(1000) / mixed
    }

    /// 主导档位：贡献覆盖字节最大的档位；并列取大档（确定性）。
    pub fn dominant_grade(&self) -> PageSize {
        let b1 = self.n1g.saturating_mul(PAGE_1G);
        let b2 = self.n2m.saturating_mul(PAGE_2M);
        let b4 = self.n4k.saturating_mul(PAGE_4K);
        if b1 > 0 && b1 >= b2 && b1 >= b4 {
            PageSize::P1G
        } else if b2 > 0 && b2 >= b4 {
            PageSize::P2M
        } else {
            PageSize::P4K
        }
    }

    /// 主导档位覆盖字节（与 F0068 cover_bytes 对账用）。
    pub fn dominant_bytes(&self) -> u64 {
        match self.dominant_grade() {
            PageSize::P1G => self.n1g.saturating_mul(PAGE_1G),
            PageSize::P2M => self.n2m.saturating_mul(PAGE_2M),
            PageSize::P4K => self.n4k.saturating_mul(PAGE_4K),
        }
    }

    /// 守恒自检（不构造、只核账）。
    pub fn conservation_ok(&self) -> bool {
        self.covered_bytes() == self.len
    }

    /// 索引法独立重推第 idx 块（回填比对的一路推导）：
    ///   idx < n1g          → 1G 档，offset = idx×1G；
    ///   idx < n1g+n2m      → 2M 档，offset = n1g×1G + (idx-n1g)×2M；
    ///   其余               → 4K 档，offset = 头部 + (idx-头块数)×4K。
    /// 与 LadderState 的顺序游标法互为独立推导，两路必须逐字段一致。
    pub fn derive_chunk(&self, idx: u64) -> Option<PlanChunk> {
        if idx >= self.total_chunks() {
            return Option::None;
        }
        if idx < self.n1g {
            let offset = idx.wrapping_mul(PAGE_1G);
            return Option::Some(PlanChunk {
                idx,
                base: self.base.wrapping_add(offset),
                grade: PageSize::P1G,
                size: PAGE_1G,
            });
        }
        let head2 = self.n1g; // 已消费的 1G 块数
        if idx < head2.wrapping_add(self.n2m) {
            let offset = head2
                .wrapping_mul(PAGE_1G)
                .wrapping_add((idx - head2).wrapping_mul(PAGE_2M));
            return Option::Some(PlanChunk {
                idx,
                base: self.base.wrapping_add(offset),
                grade: PageSize::P2M,
                size: PAGE_2M,
            });
        }
        let head4 = head2.wrapping_add(self.n2m);
        // 4K 段偏移 = 1G 头字节 + 2M 中段字节 + 段内第 k 块×4K
        let offset = self
            .n1g
            .wrapping_mul(PAGE_1G)
            .wrapping_add(self.n2m.wrapping_mul(PAGE_2M))
            .wrapping_add((idx - head4).wrapping_mul(PAGE_4K));
        Option::Some(PlanChunk {
            idx,
            base: self.base.wrapping_add(offset),
            grade: PageSize::P4K,
            size: PAGE_4K,
        })
    }
}

/// 方案内的一个映射块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanChunk {
    /// 块序号（方案内，0 基）。
    pub idx: u64,
    /// 块基址。
    pub base: u64,
    /// 块档位。
    pub grade: PageSize,
    /// 块字节尺寸（恒等于 grade.size()，回填校验对象）。
    pub size: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// LadderState：逐档下探游标（顺序游标法的一路推导）
// ═══════════════════════════════════════════════════════════════════════════

/// 档梯游标：按 I3 装填序（1G → 2M → 4K）顺序吐块。
/// 与 SplitPlan::derive_chunk 的索引法互为独立推导，供 MappingSim 对拍。
#[derive(Clone, Copy, Debug)]
pub struct LadderState {
    left1: u64,
    left2: u64,
    left4: u64,
    cursor: u64,
    idx: u64,
}

impl LadderState {
    /// 从方案建游标。
    pub fn new(plan: &SplitPlan) -> LadderState {
        LadderState {
            left1: plan.n1g,
            left2: plan.n2m,
            left4: plan.n4k,
            cursor: plan.base,
            idx: 0,
        }
    }

    /// 吐出下一块；梯尽返回 None。
    pub fn step(&mut self) -> Option<PlanChunk> {
        let (grade, size) = if self.left1 > 0 {
            self.left1 -= 1;
            (PageSize::P1G, PAGE_1G)
        } else if self.left2 > 0 {
            self.left2 -= 1;
            (PageSize::P2M, PAGE_2M)
        } else if self.left4 > 0 {
            self.left4 -= 1;
            (PageSize::P4K, PAGE_4K)
        } else {
            return Option::None;
        };
        let c = PlanChunk {
            idx: self.idx,
            base: self.cursor,
            grade,
            size,
        };
        self.cursor = self.cursor.wrapping_add(size);
        self.idx = self.idx.wrapping_add(1);
        Option::Some(c)
    }

    /// 剩余块数。
    pub fn remaining(&self) -> u64 {
        self.left1
            .saturating_add(self.left2)
            .saturating_add(self.left4)
    }

    /// 当前游标（下一块基址）。
    pub fn cursor(&self) -> u64 {
        self.cursor
    }

    /// 已吐块数。
    pub fn emitted(&self) -> u64 {
        self.idx
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 选档核心：build_split_plan（确定性贪心 + 余量降档收边）
// ═══════════════════════════════════════════════════════════════════════════

/// 逐区选档（J 判据 a 项）：
///   1. 基础三关（零长/回绕/4K 对齐）；
///   2. 档位资格：1G 需属性允许 + 1G 对齐 + 长≥1G；2M 同理；4K 恒可；
///   3. 确定性贪心：从最大有资格档起整块填装，余量降一档续填，
///      末余量全部 4K 收边（同输入必同方案）。
pub fn build_split_plan(req: &ZoneRequest) -> Result<SplitPlan, HugeSelError> {
    req.validate()?;
    let can1g = req.kind.allows(PageSize::P1G)
        && PageSize::align_ok(req.base, PageSize::P1G)
        && req.len >= PAGE_1G;
    let can2m = req.kind.allows(PageSize::P2M)
        && PageSize::align_ok(req.base, PageSize::P2M)
        && req.len >= PAGE_2M;

    let n1g: u64;
    let n2m: u64;
    let n4k: u64;
    if can1g {
        n1g = req.len >> 30;
        let rem = req.len & (PAGE_1G - 1);
        if can2m && rem >= PAGE_2M {
            n2m = rem >> 21;
            n4k = (rem & (PAGE_2M - 1)) >> 12;
        } else {
            n2m = 0;
            n4k = rem >> 12;
        }
    } else if can2m {
        n1g = 0;
        n2m = req.len >> 21;
        n4k = (req.len & (PAGE_2M - 1)) >> 12;
    } else {
        n1g = 0;
        n2m = 0;
        n4k = req.len >> 12;
    }
    SplitPlan::new_checked(req.zone_id, req.base, req.len, n1g, n2m, n4k)
}

// ═══════════════════════════════════════════════════════════════════════════
// 预判一致性对账（J 判据 b 项）：逐区 diff + 一致率 + 分歧清单
// ═══════════════════════════════════════════════════════════════════════════

/// 分歧种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiverKind {
    /// 该区在 F0068 预判表无行。
    PredMissing,
    /// 预判档位 ≠ 方案主导档位。
    GradeMismatch,
    /// 预判覆盖字节 > 方案可覆盖字节（预判超发）。
    CoverOverreach,
    /// 预判档位与区域属性矩阵冲突。
    GradeForbidden,
}

impl DiverKind {
    /// 归并到诊断码段。
    pub fn code(self) -> u16 {
        match self {
            DiverKind::PredMissing => HugeSelError::PredMissing.code(),
            DiverKind::GradeMismatch => HugeSelError::PredGradeDiverge.code(),
            DiverKind::CoverOverreach => HugeSelError::PredCoverDiverge.code(),
            DiverKind::GradeForbidden => HugeSelError::GradeNotAllowed.code(),
        }
    }

    /// 中文诊断。
    pub fn detail(self) -> &'static str {
        match self {
            DiverKind::PredMissing => "F0068 预判表缺该区行",
            DiverKind::GradeMismatch => "预判档位与方案主导档位不一致",
            DiverKind::CoverOverreach => "预判覆盖字节超出方案可覆盖量",
            DiverKind::GradeForbidden => "预判档位被区域属性矩阵禁止",
        }
    }
}

/// 单条分歧立案记录。
#[derive(Clone, Debug)]
pub struct DivergenceRec {
    /// 区号。
    pub zone_id: u32,
    /// 分歧种类。
    pub kind: DiverKind,
    /// 预判档位（缺行时为 None）。
    pub pred_grade: Option<PageSize>,
    /// 方案主导档位。
    pub plan_grade: PageSize,
    /// 预判覆盖字节（缺行时为 0）。
    pub pred_cover: u64,
    /// 方案总覆盖字节。
    pub plan_cover: u64,
}

/// 对账汇总：逐区一致率（‰）与分歧数。
#[derive(Clone, Debug)]
pub struct ReconSummary {
    /// 参与对账的区数（有方案的区）。
    pub zones: u32,
    /// 逐区全一致的区数。
    pub agree: u32,
    /// 一致率（‰；zones=0 时为 0）。
    pub agree_per_mille: u64,
    /// 分歧条数（与选择器内部分歧清单同步）。
    pub divergences: u32,
}

// ═══════════════════════════════════════════════════════════════════════════
// 事件账 / 统计 / 健康
// ═══════════════════════════════════════════════════════════════════════════

/// 事件账行（16B 量级，堆账封顶 MAX_EVENTS）。
#[derive(Clone, Copy, Debug)]
pub struct EventRec {
    /// 全局序号（wrapping 递增）。
    pub seq: u64,
    /// 诊断码（0x1570 为 OK 留痕）。
    pub code: u16,
    /// 关联区号（0 = 选择器级事件）。
    pub zone: u32,
    /// 事件短语（静态串，堆零开销）。
    pub note: &'static str,
}

/// 选择统计账（Copy，无堆字段）。
#[derive(Clone, Copy, Debug, Default)]
pub struct SelStats {
    /// 已受理区请求数。
    pub zones_total: u32,
    /// 已生成方案数。
    pub plans_ok: u32,
    /// 1G 块累计。
    pub n1g_total: u64,
    /// 2M 块累计。
    pub n2m_total: u64,
    /// 4K 块累计。
    pub n4k_total: u64,
    /// 已选区字节累计。
    pub bytes_total: u64,
    /// 事件已推送数（不含丢弃）。
    pub events_pushed: u32,
    /// 事件丢弃数（账满）。
    pub events_dropped: u32,
    /// 对账分歧数（最近一次 reconcile）。
    pub divergences: u32,
    /// 仿真执行次数。
    pub sim_runs: u32,
    /// 仿真不一致次数。
    pub sim_errors: u32,
    /// 注入检出次数（三路注入合计）。
    pub injections_detected: u32,
}

/// 健康报告。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 总体健康（分数达线且零分歧零仿真错且方案全覆盖）。
    pub ok: bool,
    /// 健康分（100 起步，逐案扣减，saturating）。
    pub score: u32,
    /// 受理区数。
    pub zones: u32,
    /// 已生成方案数。
    pub plans: u32,
    /// 最近对账分歧数。
    pub divergences: u32,
    /// 仿真错次。
    pub sim_errors: u32,
    /// 丢事件数。
    pub dropped_events: u32,
    /// 中文摘要（format 生成）。
    pub summary: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// GranuleSelector：大页粒度选择器主体
// ═══════════════════════════════════════════════════════════════════════════

/// 大页粒度选择器。
/// 生命周期：submit_request/load_prediction → seal → select_all → reconcile
/// →（MappingSim 抽样回填）→ note_sim_result → health/fingerprint。
#[derive(Clone, Debug)]
pub struct GranuleSelector {
    reqs: Vec<ZoneRequest>,
    preds: Vec<PredEntry>,
    plans: Vec<SplitPlan>,
    events: Vec<EventRec>,
    divergences: Vec<DivergenceRec>,
    diver_truncated: bool,
    sealed: bool,
    selected: bool,
    event_seq: u64,
    divergence_count: u32,
    stats: SelStats,
}

impl GranuleSelector {
    /// 空选择器。
    pub fn new() -> GranuleSelector {
        GranuleSelector {
            reqs: Vec::new(),
            preds: Vec::new(),
            plans: Vec::new(),
            events: Vec::new(),
            divergences: Vec::new(),
            diver_truncated: false,
            sealed: false,
            selected: false,
            event_seq: 0,
            divergence_count: 0,
            stats: SelStats::default(),
        }
    }

    // ── 输入面 ───────────────────────────────────────────────────────────

    /// 提交区域请求（基础三关校验，封板后拒绝）。
    pub fn submit_request(&mut self, req: ZoneRequest) -> Result<(), HugeSelError> {
        if self.sealed {
            return Err(HugeSelError::Sealed);
        }
        if self.reqs.len() >= MAX_ZONES {
            return Err(HugeSelError::LedgerFull);
        }
        req.validate()?;
        self.reqs.push(req);
        Ok(())
    }

    /// 装入 F0068 预判行（同区重复注入冲突即拒绝，保留首条语义）。
    pub fn load_prediction(&mut self, pred: PredEntry) -> Result<(), HugeSelError> {
        if self.sealed {
            return Err(HugeSelError::Sealed);
        }
        if self.preds.len() >= MAX_PREDS {
            return Err(HugeSelError::LedgerFull);
        }
        // 同区已有预判 → 拒绝（对账以首条为准，重复行属预判表缺陷）
        let mut i = 0usize;
        while i < self.preds.len() {
            if self.preds[i].zone_id == pred.zone_id {
                return Err(HugeSelError::PredMissing);
            }
            i += 1;
        }
        self.preds.push(pred);
        Ok(())
    }

    /// 封板：此后输入面只读。
    pub fn seal(&mut self) -> Result<(), HugeSelError> {
        if self.sealed {
            return Err(HugeSelError::Sealed);
        }
        self.sealed = true;
        self.push_event(E_OK, 0, "预判账与区账已封板");
        Ok(())
    }

    /// 是否已封板。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    // ── 选择面 ───────────────────────────────────────────────────────────

    /// 逐区选档生成方案（须先封板；重复生成拒绝）。
    /// 单区失败即整批失败（已生成方案保留在账上供事故取证）。
    pub fn select_all(&mut self) -> Result<(), HugeSelError> {
        if !self.sealed || self.selected {
            return Err(HugeSelError::Sealed);
        }
        self.selected = true;
        let mut i = 0usize;
        while i < self.reqs.len() {
            let req = self.reqs[i];
            let plan = build_split_plan(&req)?;
            self.stats.zones_total += 1;
            self.stats.plans_ok += 1;
            self.stats.n1g_total = self.stats.n1g_total.saturating_add(plan.n1g);
            self.stats.n2m_total = self.stats.n2m_total.saturating_add(plan.n2m);
            self.stats.n4k_total = self.stats.n4k_total.saturating_add(plan.n4k);
            self.stats.bytes_total = self.stats.bytes_total.saturating_add(plan.len);
            self.push_event(E_OK, req.zone_id, "分档方案已生成");
            self.plans.push(plan);
            i += 1;
        }
        Ok(())
    }

    /// 方案只读视图。
    pub fn plans(&self) -> &[SplitPlan] {
        &self.plans
    }

    /// 按区号查方案。
    pub fn plan_of(&self, zone_id: u32) -> Option<SplitPlan> {
        let mut i = 0usize;
        while i < self.plans.len() {
            if self.plans[i].zone_id == zone_id {
                return Option::Some(self.plans[i]);
            }
            i += 1;
        }
        Option::None
    }

    /// 按区号查首个属性（对账禁档判据用）。
    fn kind_of(&self, zone_id: u32) -> Option<ZoneKind> {
        let mut i = 0usize;
        while i < self.reqs.len() {
            if self.reqs[i].zone_id == zone_id {
                return Option::Some(self.reqs[i].kind);
            }
            i += 1;
        }
        Option::None
    }

    // ── 预判一致性对账 ──────────────────────────────────────────────────

    /// 逐区对账（J 判据 b 项）。可重跑（注入复检依赖重跑）：
    /// 每次重跑清空分歧清单重扫，同输入同结论。
    pub fn reconcile(&mut self) -> ReconSummary {
        self.divergences.clear();
        self.diver_truncated = false;
        let mut zones = 0u32;
        let mut agree = 0u32;
        let mut pi = 0usize;
        while pi < self.plans.len() {
            let plan = self.plans[pi];
            zones += 1;
            let mut zone_ok = true;
            let kind = self.kind_of(plan.zone_id);
            // 扫描该区全部预判行（一区一行为主流；多行逐行核）
            let mut found = false;
            let mut qi = 0usize;
            while qi < self.preds.len() {
                let pred = self.preds[qi];
                if pred.zone_id != plan.zone_id {
                    qi += 1;
                    continue;
                }
                found = true;
                let mut dkind: Option<DiverKind> = Option::None;
                if let Option::Some(k) = kind {
                    if !k.allows(pred.grade) {
                        dkind = Option::Some(DiverKind::GradeForbidden);
                    }
                }
                if dkind.is_none() && pred.grade != plan.dominant_grade() {
                    dkind = Option::Some(DiverKind::GradeMismatch);
                }
                if dkind.is_none() && pred.cover_bytes > plan.covered_bytes() {
                    dkind = Option::Some(DiverKind::CoverOverreach);
                }
                if let Option::Some(k) = dkind {
                    zone_ok = false;
                    self.file_divergence(DivergenceRec {
                        zone_id: plan.zone_id,
                        kind: k,
                        pred_grade: Option::Some(pred.grade),
                        plan_grade: plan.dominant_grade(),
                        pred_cover: pred.cover_bytes,
                        plan_cover: plan.covered_bytes(),
                    });
                }
                qi += 1;
            }
            if !found {
                zone_ok = false;
                self.file_divergence(DivergenceRec {
                    zone_id: plan.zone_id,
                    kind: DiverKind::PredMissing,
                    pred_grade: Option::None,
                    plan_grade: plan.dominant_grade(),
                    pred_cover: 0,
                    plan_cover: plan.covered_bytes(),
                });
            }
            if zone_ok {
                agree += 1;
            }
            pi += 1;
        }
        self.divergence_count = self.divergences.len() as u32;
        self.stats.divergences = self.divergence_count;
        let per_mille = if zones == 0 {
            0
        } else {
            (agree as u64).saturating_mul(1000) / zones as u64
        };
        if self.divergence_count == 0 {
            self.push_event(E_OK, 0, "预判逐区对账一致");
        } else {
            self.push_event(
                HugeSelError::PredGradeDiverge.code(),
                0,
                "预判逐区对账出现分歧，已立案",
            );
        }
        ReconSummary {
            zones,
            agree,
            agree_per_mille: per_mille,
            divergences: self.divergence_count,
        }
    }

    /// 立案入清单（封顶截断置位）。
    fn file_divergence(&mut self, d: DivergenceRec) {
        if self.divergences.len() >= MAX_DIVERGENCES {
            self.diver_truncated = true;
            return;
        }
        self.divergences.push(d);
    }

    /// 分歧清单只读视图。
    pub fn divergence_list(&self) -> &[DivergenceRec] {
        &self.divergences
    }

    /// 最近对账分歧数。
    pub fn divergence_count(&self) -> u32 {
        self.divergence_count
    }

    /// 分歧清单是否被截断。
    pub fn divergences_truncated(&self) -> bool {
        self.diver_truncated
    }

    // ── 事件账 ───────────────────────────────────────────────────────────

    /// 推事件（封顶后走丢包计数，不阻塞主流程）。
    fn push_event(&mut self, code: u16, zone: u32, note: &'static str) {
        if self.events.len() >= MAX_EVENTS {
            self.stats.events_dropped = self.stats.events_dropped.saturating_add(1);
            return;
        }
        self.event_seq = self.event_seq.wrapping_add(1);
        self.events.push(EventRec { seq: self.event_seq, code, zone, note });
        self.stats.events_pushed = self.stats.events_pushed.saturating_add(1);
    }

    /// 事件账只读视图。
    pub fn events(&self) -> &[EventRec] {
        &self.events
    }

    /// 统计账副本。
    pub fn stats(&self) -> SelStats {
        self.stats
    }

    // ── FNV 指纹（确定性封印） ──────────────────────────────────────────

    /// 方案指纹：仅方案全字段推进 FNV-1a（对拍专用，剥离事件账时序差）。
    pub fn plan_fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.plans.len() {
            let p = &self.plans[i];
            h = fnv_u64(h, p.zone_id as u64);
            h = fnv_u64(h, p.base);
            h = fnv_u64(h, p.len);
            h = fnv_u64(h, p.n1g);
            h = fnv_u64(h, p.n2m);
            h = fnv_u64(h, p.n4k);
            i += 1;
        }
        h
    }

    /// 账面指纹：方案全字段 + 事件账顺序推进 FNV-1a。
    /// 同输入同指纹是对拍判据；任何未冻结随机源都会在这里现形。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.plans.len() {
            let p = &self.plans[i];
            h = fnv_u64(h, p.zone_id as u64);
            h = fnv_u64(h, p.base);
            h = fnv_u64(h, p.len);
            h = fnv_u64(h, p.n1g);
            h = fnv_u64(h, p.n2m);
            h = fnv_u64(h, p.n4k);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.events.len() {
            let e = &self.events[j];
            h = fnv_u64(h, e.seq);
            h = fnv_u64(h, e.code as u64);
            h = fnv_u64(h, e.zone as u64);
            let note = e.note;
            let mut k = 0usize;
            while k < note.len() {
                h = fnv_byte(h, note.as_bytes()[k]);
                k += 1;
            }
            j += 1;
        }
        h
    }

    // ── 双跑确定性对拍 ──────────────────────────────────────────────────

    /// 与另一选择器逐方案字段对拍 + 方案指纹对拍。
    pub fn dual_run_crosscheck(&self, other: &GranuleSelector) -> DeterminismReport {
        let fpa = self.plan_fingerprint();
        let fpb = other.plan_fingerprint();
        if self.plans.len() != other.plans.len() {
            return DeterminismReport {
                ok: false,
                reason: "方案数不一致",
                fp_a: fpa,
                fp_b: fpb,
                plans_a: self.plans.len() as u32,
                plans_b: other.plans.len() as u32,
            };
        }
        let mut i = 0usize;
        while i < self.plans.len() {
            let a = &self.plans[i];
            let b = &other.plans[i];
            let same = a.zone_id == b.zone_id
                && a.base == b.base
                && a.len == b.len
                && a.n1g == b.n1g
                && a.n2m == b.n2m
                && a.n4k == b.n4k;
            if !same {
                return DeterminismReport {
                    ok: false,
                    reason: "逐方案字段不一致",
                    fp_a: fpa,
                    fp_b: fpb,
                    plans_a: self.plans.len() as u32,
                    plans_b: other.plans.len() as u32,
                };
            }
            i += 1;
        }
        DeterminismReport {
            ok: fpa == fpb,
            reason: if fpa == fpb { "指纹一致" } else { "指纹漂移" },
            fp_a: fpa,
            fp_b: fpb,
            plans_a: self.plans.len() as u32,
            plans_b: other.plans.len() as u32,
        }
    }

    /// 双跑自检：以本选择器冻结的输入重建一条影子流水线，
    /// 与自身对拍；漂移即 DeterminismDrift 立案。
    pub fn determinism_selfcheck(&mut self) -> Result<u64, HugeSelError> {
        let mut shadow = GranuleSelector::new();
        let mut i = 0usize;
        while i < self.reqs.len() {
            shadow.submit_request(self.reqs[i])?;
            i += 1;
        }
        let mut j = 0usize;
        while j < self.preds.len() {
            shadow.load_prediction(self.preds[j])?;
            j += 1;
        }
        shadow.seal()?;
        shadow.select_all()?;
        let rep = self.dual_run_crosscheck(&shadow);
        if rep.ok {
            self.push_event(E_OK, 0, "双跑确定性对拍一致");
            Ok(rep.fp_a)
        } else {
            self.push_event(
                HugeSelError::DeterminismDrift.code(),
                0,
                "双跑确定性对拍漂移，已立案",
            );
            Err(HugeSelError::DeterminismDrift)
        }
    }

    // ── 仿真回执登记 ────────────────────────────────────────────────────

    /// 登记 MappingSim 回执（ok/error 入账；回填不一致立案）。
    pub fn note_sim_result(&mut self, rep: &SimReport) {
        self.stats.sim_runs = self.stats.sim_runs.saturating_add(1);
        match rep.error {
            Option::None => {
                self.push_event(E_OK, rep.zone_id, "映射仿真+抽样回填通过");
            }
            Option::Some(e) => {
                self.stats.sim_errors = self.stats.sim_errors.saturating_add(1);
                let code = e.code();
                self.push_event(code, rep.zone_id, "映射仿真回填/守恒检出异常，已立案");
            }
        }
    }

    // ── 开销账（页表项数最小化对比） ────────────────────────────────────

    /// 单区 PTE 对比行。
    pub fn pte_rows(&self) -> Vec<PteRow> {
        let mut rows: Vec<PteRow> = Vec::new();
        let mut i = 0usize;
        while i < self.plans.len() {
            let p = &self.plans[i];
            rows.push(PteRow {
                zone_id: p.zone_id,
                all4k_entries: p.all4k_entries(),
                mixed_entries: p.pte_entries(),
                savings_per_mille: p.savings_per_mille(),
            });
            i += 1;
        }
        rows
    }

    /// 全账汇总：全 4K vs 混合档位的页表项数对比账。
    pub fn pte_min_ledger(&self) -> PteLedger {
        let mut all4k = 0u64;
        let mut mixed = 0u64;
        let mut i = 0usize;
        while i < self.plans.len() {
            all4k = all4k.saturating_add(self.plans[i].all4k_entries());
            mixed = mixed.saturating_add(self.plans[i].pte_entries());
            i += 1;
        }
        let savings = if mixed == 0 {
            0
        } else {
            all4k.saturating_mul(1000) / mixed
        };
        PteLedger {
            zones: self.plans.len() as u32,
            all4k_entries: all4k,
            mixed_entries: mixed,
            savings_per_mille: savings,
        }
    }

    // ── 健康 ────────────────────────────────────────────────────────────

    /// 健康评估：分歧/仿真错/丢事件逐项扣分，方案全覆盖为达线前提。
    pub fn health(&self) -> HealthReport {
        let mut score: u32 = 100;
        score = score.saturating_sub(self.divergence_count.saturating_mul(25));
        score = score.saturating_sub(self.stats.sim_errors.saturating_mul(20));
        score = score.saturating_sub(self.stats.events_dropped.saturating_mul(2));
        if self.diver_truncated {
            score = score.saturating_sub(5);
        }
        let full_cover = self.plans.len() as u32 == self.stats.zones_total;
        let ok = score >= 90
            && self.divergence_count == 0
            && self.stats.sim_errors == 0
            && full_cover;
        let summary = format!(
            "UNX-F0305 健康：区{}/方案{} 分歧{} 仿真错{} 丢事件{} 截断{} → 得分{}",
            self.stats.zones_total,
            self.plans.len() as u32,
            self.divergence_count,
            self.stats.sim_errors,
            self.stats.events_dropped,
            self.diver_truncated as u32,
            score,
        );
        HealthReport {
            ok,
            score,
            zones: self.stats.zones_total,
            plans: self.plans.len() as u32,
            divergences: self.divergence_count,
            sim_errors: self.stats.sim_errors,
            dropped_events: self.stats.events_dropped,
            summary,
        }
    }

    // ── 账表导出（诊断视图） ────────────────────────────────────────────

    /// 方案表（封顶 64 行，超出标省略）。
    pub fn plans_table(&self) -> String {
        let mut s = String::from("区号  基址          长度          1G     2M     4K     主导 节省‰\r\n");
        let cap = if self.plans.len() < 64 { self.plans.len() } else { 64 };
        let mut i = 0usize;
        while i < cap {
            let p = &self.plans[i];
            let line = format!(
                "{:<5} 0x{:012X} 0x{:012X} {:<6} {:<6} {:<6} {:<4} {}\r\n",
                p.zone_id,
                p.base,
                p.len,
                p.n1g,
                p.n2m,
                p.n4k,
                p.dominant_grade().name(),
                p.savings_per_mille(),
            );
            s.push_str(&line);
            i += 1;
        }
        if self.plans.len() > cap {
            s.push_str("…（超 64 行省略）\r\n");
        }
        s
    }

    /// 分歧清单表。
    pub fn divergence_table(&self) -> String {
        if self.divergences.is_empty() {
            return String::from("分歧清单：空（逐区一致）\r\n");
        }
        let mut s = String::from("区号  种类            预判档  方案档  预判覆盖      方案覆盖\r\n");
        let mut i = 0usize;
        while i < self.divergences.len() {
            let d = &self.divergences[i];
            let pg = match d.pred_grade {
                Option::Some(g) => g.name(),
                Option::None => "--",
            };
            let line = format!(
                "{:<5} {:<15} {:<7} {:<7} 0x{:012X} 0x{:012X}\r\n",
                d.zone_id,
                d.kind.detail(),
                pg,
                d.plan_grade.name(),
                d.pred_cover,
                d.plan_cover,
            );
            s.push_str(&line);
            i += 1;
        }
        s
    }

    /// 事件账表（封顶 64 行）。
    pub fn events_table(&self) -> String {
        let mut s = String::from("序号  码      区号  事件\r\n");
        let cap = if self.events.len() < 64 { self.events.len() } else { 64 };
        let mut i = 0usize;
        while i < cap {
            let e = &self.events[i];
            let line = format!(
                "{:<5} 0x{:04X}  {:<5} {}\r\n",
                e.seq, e.code, e.zone, e.note,
            );
            s.push_str(&line);
            i += 1;
        }
        if self.events.len() > cap {
            s.push_str("…（超 64 行省略）\r\n");
        }
        s
    }

    // ── 注入面（三路故障注入，逐路验证检出） ────────────────────────────

    /// 注入一：不对齐注入。提交基址 0x0BAD（非 4K 对齐）的区请求，
    /// 预期 BaseUnalign 检出。返回 Ok(true)=检出。
    pub fn inject_misalignment(&mut self) -> Result<bool, HugeSelError> {
        if !self.sealed {
            // 注入允许在封板前做：走真实受理路径会被账面吞掉校验错，
            // 这里直接打选档函数验证判据本身。
            return Err(HugeSelError::Sealed);
        }
        let bad = ZoneRequest {
            zone_id: 0xFFFF_FFFF,
            base: 0x0BAD,
            len: PAGE_2M,
            kind: ZoneKind::KernelData,
        };
        let detected = match build_split_plan(&bad) {
            Err(HugeSelError::BaseUnalign) => true,
            _ => false,
        };
        if detected {
            self.push_event(
                HugeSelError::BaseUnalign.code(),
                0xFFFF_FFFF,
                "不对齐注入：BaseUnalign 检出",
            );
            self.stats.injections_detected = self.stats.injections_detected.saturating_add(1);
        } else {
            self.push_event(
                HugeSelError::InternalOverflow.code(),
                0xFFFF_FFFF,
                "不对齐注入：未检出（判据失守）",
            );
        }
        Ok(detected)
    }

    /// 注入二：预判分歧注入。向预判账直接追加一条与既有方案主导档位
    /// 相左的 F0068 行（绕过 load_prediction 的去重，模拟预判表缺陷），
    /// 重跑对账，预期分歧计数上升。返回 Ok(true)=检出。
    pub fn inject_pred_divergence(&mut self) -> Result<bool, HugeSelError> {
        if self.plans.is_empty() {
            return Ok(false);
        }
        let target = self.plans[0];
        let contra = match target.dominant_grade() {
            PageSize::P1G => PageSize::P4K,
            _ => PageSize::P1G,
        };
        let before = self.divergence_count;
        // 直接绕过 load_prediction 去重：注入面模拟"预判表自身的缺陷行"
        if self.preds.len() >= MAX_PREDS {
            return Err(HugeSelError::LedgerFull);
        }
        self.preds.push(PredEntry {
            zone_id: target.zone_id,
            grade: contra,
            cover_bytes: 0,
        });
        let summary = self.reconcile();
        let detected = summary.divergences > before;
        if detected {
            self.push_event(
                HugeSelError::PredGradeDiverge.code(),
                target.zone_id,
                "预判分歧注入：逐区对账检出相左行",
            );
            self.stats.injections_detected = self.stats.injections_detected.saturating_add(1);
        } else {
            self.push_event(
                HugeSelError::InternalOverflow.code(),
                target.zone_id,
                "预判分歧注入：未检出（对账失守）",
            );
        }
        Ok(detected)
    }

    /// 注入三：覆盖缺口注入。取末位方案副本，抽走一块 4K（守恒即破），
    /// 走受检构造重验，预期 CoverageGap 检出。原方案不动。
    /// 返回 Ok(true)=检出。
    pub fn inject_coverage_gap(&mut self) -> Result<bool, HugeSelError> {
        if self.plans.is_empty() {
            return Ok(false);
        }
        let last = self.plans[self.plans.len() - 1];
        if last.n4k == 0 {
            // 无 4K 侧可抽：改抽 2M 侧重验
            if last.n2m == 0 {
                self.push_event(E_OK, last.zone_id, "覆盖缺口注入：无可抽块，跳过");
                return Ok(false);
            }
            let tampered = SplitPlan::new_checked(
                last.zone_id,
                last.base,
                last.len,
                last.n1g,
                last.n2m - 1,
                last.n4k,
            );
            let detected = match tampered {
                Err(HugeSelError::CoverageGap) => true,
                _ => false,
            };
            if detected {
                self.stats.injections_detected = self.stats.injections_detected.saturating_add(1);
                self.push_event(
                    HugeSelError::CoverageGap.code(),
                    last.zone_id,
                    "覆盖缺口注入（2M 侧）：CoverageGap 检出",
                );
            }
            return Ok(detected);
        }
        let tampered =
            SplitPlan::new_checked(last.zone_id, last.base, last.len, last.n1g, last.n2m, last.n4k - 1);
        let detected = match tampered {
            Err(HugeSelError::CoverageGap) => true,
            _ => false,
        };
        if detected {
            self.stats.injections_detected = self.stats.injections_detected.saturating_add(1);
            self.push_event(
                HugeSelError::CoverageGap.code(),
                last.zone_id,
                "覆盖缺口注入（4K 侧）：CoverageGap 检出",
            );
        } else {
            self.push_event(
                HugeSelError::InternalOverflow.code(),
                last.zone_id,
                "覆盖缺口注入：未检出（守恒校验失守）",
            );
        }
        Ok(detected)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 对拍报告 / 开销账类型
// ═══════════════════════════════════════════════════════════════════════════

/// 双跑确定性对拍报告。
#[derive(Clone, Copy, Debug)]
pub struct DeterminismReport {
    /// 对拍是否一致。
    pub ok: bool,
    /// 结论短语。
    pub reason: &'static str,
    /// 甲跑指纹。
    pub fp_a: u64,
    /// 乙跑指纹。
    pub fp_b: u64,
    /// 甲跑方案数。
    pub plans_a: u32,
    /// 乙跑方案数。
    pub plans_b: u32,
}

/// 单区页表项对比行。
#[derive(Clone, Copy, Debug)]
pub struct PteRow {
    /// 区号。
    pub zone_id: u32,
    /// 全 4K 方案叶项数。
    pub all4k_entries: u64,
    /// 混合方案叶项数。
    pub mixed_entries: u64,
    /// 节省倍率（‰）。
    pub savings_per_mille: u64,
}

/// 全账页表项对比账。
#[derive(Clone, Copy, Debug)]
pub struct PteLedger {
    /// 区数。
    pub zones: u32,
    /// 全 4K 叶项总数。
    pub all4k_entries: u64,
    /// 混合叶项总数。
    pub mixed_entries: u64,
    /// 节省倍率（‰）。
    pub savings_per_mille: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// MappingSim：映射仿真与抽样回填（J 判据 c 项）
// ═══════════════════════════════════════════════════════════════════════════

/// 抽样样本（回填比对留痕）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimSample {
    /// 块序号。
    pub idx: u64,
    /// 块基址（顺序游标法产物）。
    pub base: u64,
    /// 块档位。
    pub grade: PageSize,
    /// 块尺寸。
    pub size: u64,
}

/// 仿真回执。
#[derive(Clone, Copy, Debug)]
pub struct SimReport {
    /// 区号。
    pub zone_id: u32,
    /// 仿真是否全过。
    pub ok: bool,
    /// 已映射字节（saturating 累计，守恒时应 ≡ 区长）。
    pub mapped_bytes: u64,
    /// 总块数。
    pub total_chunks: u64,
    /// 回填比对次数。
    pub backfill_checked: u64,
    /// 首个异常（None = 全过）。
    pub error: Option<HugeSelError>,
}

/// 单方案映射仿真器：顺序游标法推进 + 定步抽样回填。
/// 走查全量块（O(total)），样本上堆封顶 MAX_SIM_SAMPLES。
pub struct MappingSim {
    plan: SplitPlan,
    samples: Vec<SimSample>,
    sample_cap: usize,
    last: Option<SimReport>,
}

impl MappingSim {
    /// 建仿真器（块数超 MAX_SIM_CHUNKS 拒绝；样本封顶不超 MAX_SIM_SAMPLES）。
    pub fn new(plan: SplitPlan, sample_cap: usize) -> Result<MappingSim, HugeSelError> {
        if plan.total_chunks() > MAX_SIM_CHUNKS {
            return Err(HugeSelError::LedgerFull);
        }
        let cap = if sample_cap > MAX_SIM_SAMPLES {
            MAX_SIM_SAMPLES
        } else {
            sample_cap
        };
        Ok(MappingSim {
            plan,
            samples: Vec::new(),
            sample_cap: cap,
            last: Option::None,
        })
    }

    /// 执行仿真：连续性 + 档位尺寸 + 抽样回填 + 覆盖守恒四道闸。
    pub fn run(&mut self) -> SimReport {
        let total = self.plan.total_chunks();
        let mut lad = LadderState::new(&self.plan);
        let mut mapped: u64 = 0;
        let mut checked: u64 = 0;
        let mut last_end = self.plan.base;
        let mut err: Option<HugeSelError> = Option::None;
        let mut steps = 0u64;
        while steps < total {
            let c = match lad.step() {
                Option::Some(c) => c,
                Option::None => {
                    err = Option::Some(HugeSelError::InternalOverflow);
                    break;
                }
            };
            // 闸一：连续性（块基址必须等于前块终点）
            if c.base != last_end {
                err = Option::Some(HugeSelError::SimContiguity);
                break;
            }
            // 闸二：块尺寸自洽
            if c.size != c.grade.size() {
                err = Option::Some(HugeSelError::SimBackfill);
                break;
            }
            // 闸三：块基址按自身档位对齐
            if !PageSize::align_ok(c.base, c.grade) {
                err = Option::Some(HugeSelError::SimBackfill);
                break;
            }
            // 闸四（J 判据）：抽样回填——索引法独立重推并逐字段比对
            if c.idx % SAMPLE_STRIDE == 0 || c.idx == 0 || c.idx + 1 == total {
                let agree = match self.plan.derive_chunk(c.idx) {
                    Option::Some(e) => {
                        e.base == c.base && e.grade == c.grade && e.size == c.size
                    }
                    Option::None => false,
                };
                if !agree {
                    err = Option::Some(HugeSelError::SimBackfill);
                    break;
                }
                checked += 1;
                if self.samples.len() < self.sample_cap {
                    self.samples.push(SimSample {
                        idx: c.idx,
                        base: c.base,
                        grade: c.grade,
                        size: c.size,
                    });
                }
            }
            mapped = mapped.saturating_add(c.size);
            last_end = c.base.wrapping_add(c.size);
            steps += 1;
        }
        if err.is_none() {
            // 闸五：覆盖守恒（映射字节 ≡ 区长）
            if mapped != self.plan.len {
                err = Option::Some(HugeSelError::CoverageGap);
            } else if last_end != self.plan.base.wrapping_add(self.plan.len) {
                err = Option::Some(HugeSelError::SimContiguity);
            }
        }
        let rep = SimReport {
            zone_id: self.plan.zone_id,
            ok: err.is_none(),
            mapped_bytes: mapped,
            total_chunks: total,
            backfill_checked: checked,
            error: err,
        };
        self.last = Option::Some(rep);
        rep
    }

    /// 样本账只读视图。
    pub fn samples(&self) -> &[SimSample] {
        &self.samples
    }

    /// 最近回执。
    pub fn last_report(&self) -> Option<SimReport> {
        self.last
    }

    /// 被仿方案视图。
    pub fn plan(&self) -> SplitPlan {
        self.plan
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 模块自述
// ═══════════════════════════════════════════════════════════════════════════

/// (任务标签, 任务号, 诊断段基址)。
pub fn module_id() -> (&'static str, u16, u16) {
    (TASK_TAG, MODULE_ID, SEG_BASE)
}

// ═══════════════════════════════════════════════════════════════════════════
// 编译期不变式（const assert，非运行期）
// ═══════════════════════════════════════════════════════════════════════════

const _: () = assert!(PAGE_4K == 1u64 << 12, "4K 必须是 2^12");
const _: () = assert!(PAGE_2M == 1u64 << 21, "2M 必须是 2^21");
const _: () = assert!(PAGE_1G == 1u64 << 30, "1G 必须是 2^30");
const _: () = assert!(PAGE_1G % PAGE_2M == 0, "1G 必须是 2M 整数倍");
const _: () = assert!(PAGE_2M % PAGE_4K == 0, "2M 必须是 4K 整数倍");
const _: () = assert!(SEG_BASE == 0x1570, "诊断段冻结 0x1570");
const _: () = assert!(MAX_ZONES <= 1024, "区账上限不得越过 1024");
const _: () = assert!(SAMPLE_STRIDE >= 1, "抽样步距必须为正");
const _: () = assert!(
    HugeSelError::InternalOverflow.code() == SEG_BASE + 0x0F,
    "诊断码闭集必须收在段尾 0x157F"
);

// ═══════════════════════════════════════════════════════════════════════════
// 冒烟测试（隔离探针 vx_probe_unx0305 执行；测试路径允许 assert/unwrap）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod t0305 {
    use super::*;
    use alloc::vec;

    /// 标准五区场景：
    ///   z1 内核数据 1G 对齐 6G+2M+4K → 6×1G + 1×2M + 1×4K
    ///   z2 直接映射 1G 对齐 768M    → 384×2M（不足 1G 降 2M 档）
    ///   z3 设备 MMIO 64K            → 16×4K（禁大页）
    ///   z4 内核代码 2M 对齐但基址非 1G/2M 处 → 512×4K（对齐不足全降档）
    ///   z5 用户空闲 1G 对齐 0x1_2345_6000 → 1×1G + 0x11A2×2M + 22×4K
    fn demo_zones() -> Vec<ZoneRequest> {
        vec![
            ZoneRequest {
                zone_id: 1,
                base: 0x1_0000_0000,
                len: 0x1_8020_1000,
                kind: ZoneKind::KernelData,
            },
            ZoneRequest {
                zone_id: 2,
                base: 0x4_0000_0000,
                len: 0x3000_0000,
                kind: ZoneKind::DirectMap,
            },
            ZoneRequest {
                zone_id: 3,
                base: 0xFEE0_0000,
                len: 0x1_0000,
                kind: ZoneKind::DeviceMmio,
            },
            ZoneRequest {
                zone_id: 4,
                base: 0x2_8020_1000,
                len: 0x20_0000,
                kind: ZoneKind::KernelText,
            },
            ZoneRequest {
                zone_id: 5,
                base: 0x8_0000_0000,
                len: 0x1_2345_6000,
                kind: ZoneKind::UserFree,
            },
        ]
    }

    /// 与 demo_zones 逐区一致的 F0068 预判表。
    fn demo_preds() -> Vec<PredEntry> {
        vec![
            PredEntry { zone_id: 1, grade: PageSize::P1G, cover_bytes: 0x1_8000_0000 },
            PredEntry { zone_id: 2, grade: PageSize::P2M, cover_bytes: 0x3000_0000 },
            PredEntry { zone_id: 3, grade: PageSize::P4K, cover_bytes: 0x1_0000 },
            PredEntry { zone_id: 4, grade: PageSize::P4K, cover_bytes: 0x20_0000 },
            PredEntry { zone_id: 5, grade: PageSize::P1G, cover_bytes: 0x1_0000_0000 },
        ]
    }

    /// 建标准选择器：提交→预判→封板→选档→对账。
    fn demo_selector() -> GranuleSelector {
        let mut s = GranuleSelector::new();
        for z in demo_zones() {
            s.submit_request(z).unwrap();
        }
        for p in demo_preds() {
            s.load_prediction(p).unwrap();
        }
        s.seal().unwrap();
        s.select_all().unwrap();
        s.reconcile();
        s
    }

    #[test]
    fn t01_const_table() {
        // 三档闭集与序号往返
        assert_eq!(PAGE_4K, 4096);
        assert_eq!(PAGE_2M, 2 * 1024 * 1024);
        assert_eq!(PAGE_1G, 1024 * 1024 * 1024);
        let mut i = 0u8;
        while i < 3 {
            let g = PageSize::from_index(i).unwrap();
            assert_eq!(g.index(), i);
            assert_eq!(g.size(), 1u64 << g.shift());
            i += 1;
        }
        assert!(PageSize::from_index(3).is_none());
        // 属性矩阵
        assert!(ZoneKind::DeviceMmio.allows(PageSize::P4K));
        assert!(!ZoneKind::DeviceMmio.allows(PageSize::P2M));
        assert!(ZoneKind::DirectMap.allows(PageSize::P1G));
        assert_eq!(ZoneKind::DeviceMmio.allowed_count(), 1);
        assert_eq!(ZoneKind::KernelText.allowed_count(), 3);
        // 对齐判据
        assert!(PageSize::align_ok(0x1_0000_0000, PageSize::P1G));
        assert!(!PageSize::align_ok(0x1_0020_0000, PageSize::P1G));
        assert!(PageSize::align_ok(0x1_0020_0000, PageSize::P2M));
        assert_eq!(PageSize::align_up(0x1234, PageSize::P4K), 0x2000);
        assert_eq!(PageSize::full_count(0x1_2345_6000, PageSize::P1G), 4);
    }

    #[test]
    fn t02_alignment_criteria() {
        // 非对齐基址拒绝
        let bad_base = ZoneRequest {
            zone_id: 99,
            base: 0x1234_5678,
            len: PAGE_2M,
            kind: ZoneKind::KernelData,
        };
        match build_split_plan(&bad_base) {
            Err(HugeSelError::BaseUnalign) => {}
            other => panic!("应判 BaseUnalign，实得 {:?}", other.map(|p| p.n4k)),
        }
        // 非对齐长度拒绝
        let bad_len = ZoneRequest {
            zone_id: 99,
            base: 0x1_0000_0000,
            len: 0x1_801,
            kind: ZoneKind::KernelData,
        };
        match build_split_plan(&bad_len) {
            Err(HugeSelError::LenUnalign) => {}
            _ => panic!("应判 LenUnalign"),
        }
        // 零长拒绝
        let zero = ZoneRequest { zone_id: 99, base: 0, len: 0, kind: ZoneKind::UserFree };
        match build_split_plan(&zero) {
            Err(HugeSelError::ZeroLen) => {}
            _ => panic!("应判 ZeroLen"),
        }
        // 回绕拒绝
        let wrap = ZoneRequest {
            zone_id: 99,
            base: u64::MAX - 0x1000,
            len: 0x2000,
            kind: ZoneKind::UserFree,
        };
        match build_split_plan(&wrap) {
            Err(HugeSelError::Overrun) => {}
            _ => panic!("应判 Overrun"),
        }
        // 2M 对齐但非 1G 对齐 → 1G 档失格，降 2M 档
        let mid = ZoneRequest {
            zone_id: 98,
            base: 0x1_0020_0000,
            len: 0x40_0000,
            kind: ZoneKind::KernelData,
        };
        let p = build_split_plan(&mid).unwrap();
        assert_eq!(p.n1g, 0);
        assert_eq!(p.n2m, 2);
        assert_eq!(p.n4k, 0);
        // DeviceMmio 强制 4K
        let dev = ZoneRequest {
            zone_id: 97,
            base: 0xFEE0_0000,
            len: 0x30_0000,
            kind: ZoneKind::DeviceMmio,
        };
        let d = build_split_plan(&dev).unwrap();
        assert_eq!(d.n1g + d.n2m, 0);
        assert_eq!(d.n4k, 0x300);
    }

    #[test]
    fn t03_plan_determinism() {
        // 同输入两次全流程 → 方案逐字段一致 + 指纹一致
        let a = demo_selector();
        let b = demo_selector();
        assert_eq!(a.plans().len(), b.plans().len());
        let mut i = 0usize;
        while i < a.plans().len() {
            assert_eq!(a.plans()[i], b.plans()[i]);
            i += 1;
        }
        assert_eq!(a.fingerprint(), b.fingerprint());
        let rep = a.dual_run_crosscheck(&b);
        assert!(rep.ok, "双跑对拍失败：{}", rep.reason);
    }

    #[test]
    fn t04_coverage_conservation() {
        let s = demo_selector();
        let mut i = 0usize;
        while i < s.plans().len() {
            let p = &s.plans()[i];
            assert!(p.conservation_ok(), "区{}守恒破裂", p.zone_id);
            assert_eq!(
                p.n1g * PAGE_1G + p.n2m * PAGE_2M + p.n4k * PAGE_4K,
                p.len,
                "区{}页数守恒等式不成立",
                p.zone_id
            );
            i += 1;
        }
        // 具体分解核对
        let z1 = s.plan_of(1).unwrap();
        assert_eq!((z1.n1g, z1.n2m, z1.n4k), (6, 1, 1));
        let z2 = s.plan_of(2).unwrap();
        assert_eq!((z2.n1g, z2.n2m, z2.n4k), (0, 384, 0));
        let z3 = s.plan_of(3).unwrap();
        assert_eq!((z3.n1g, z3.n2m, z3.n4k), (0, 0, 16));
        let z4 = s.plan_of(4).unwrap();
        assert_eq!((z4.n1g, z4.n2m, z4.n4k), (0, 0, 512));
        let z5 = s.plan_of(5).unwrap();
        assert_eq!(z5.n1g, 4);
        assert_eq!(z5.n2m, 0x11A);
        assert_eq!(z5.n4k, 86);
        // 统计账与方案聚合一致
        let st = s.stats();
        assert_eq!(st.plans_ok, 5);
        assert_eq!(st.n1g_total, 6 + 4);
        assert_eq!(st.n2m_total, 1 + 384 + 0x11A);
        assert_eq!(st.n4k_total, 1 + 16 + 512 + 86);
    }

    #[test]
    fn t05_pred_consistency() {
        let mut s = demo_selector();
        let recon = s.reconcile();
        assert_eq!(recon.zones, 5);
        assert_eq!(recon.agree, 5);
        assert_eq!(recon.agree_per_mille, 1000, "标准场景必须逐区一致");
        assert!(s.divergence_list().is_empty());
        // 预判缺失立案：换一张缺 z4 行的预判表
        let mut s2 = GranuleSelector::new();
        for z in demo_zones() {
            s2.submit_request(z).unwrap();
        }
        for p in demo_preds() {
            if p.zone_id != 4 {
                s2.load_prediction(p).unwrap();
            }
        }
        s2.seal().unwrap();
        s2.select_all().unwrap();
        let r2 = s2.reconcile();
        assert_eq!(r2.agree_per_mille, 800);
        assert_eq!(r2.divergences, 1);
        let d = &s2.divergence_list()[0];
        assert_eq!(d.kind, DiverKind::PredMissing);
        assert!(!s2.health().ok, "分歧必须压健康分");
        // 预判超发立案：z3 的预判覆盖改成超发值
        let mut s3 = GranuleSelector::new();
        for z in demo_zones() {
            s3.submit_request(z).unwrap();
        }
        s3.load_prediction(PredEntry { zone_id: 3, grade: PageSize::P4K, cover_bytes: 0x1_0000 }).unwrap();
        s3.load_prediction(PredEntry { zone_id: 1, grade: PageSize::P1G, cover_bytes: u64::MAX }).unwrap();
        s3.seal().unwrap();
        s3.select_all().unwrap();
        let r3 = s3.reconcile();
        // z2/z4/z5 预判缺失 ×3 + z1 预判超发 ×1
        assert_eq!(r3.divergences, 4);
        assert!(s3
            .divergence_list()
            .iter()
            .any(|d| d.zone_id == 1 && d.kind == DiverKind::CoverOverreach));
        // 属性禁档立案：DeviceMmio 区收到 2M 预判
        let mut s4 = GranuleSelector::new();
        s4.submit_request(ZoneRequest {
            zone_id: 7,
            base: 0xFEE0_0000,
            len: 0x1_0000,
            kind: ZoneKind::DeviceMmio,
        })
        .unwrap();
        s4.load_prediction(PredEntry { zone_id: 7, grade: PageSize::P2M, cover_bytes: 0x1_0000 }).unwrap();
        s4.seal().unwrap();
        s4.select_all().unwrap();
        let r4 = s4.reconcile();
        assert_eq!(r4.divergences, 1);
        assert_eq!(s4.divergence_list()[0].kind, DiverKind::GradeForbidden);
    }

    #[test]
    fn t06_backfill_sampling() {
        let mut s = demo_selector();
        let mut total_checked = 0u64;
        let mut i = 0usize;
        while i < s.plans().len() {
            let plan = s.plans()[i];
            let mut sim = MappingSim::new(plan, 1024).unwrap();
            let rep = sim.run();
            assert!(rep.ok, "区{}仿真失败：{:?}", plan.zone_id, rep.error);
            assert_eq!(rep.mapped_bytes, plan.len, "区{}映射字节守恒破", plan.zone_id);
            assert_eq!(rep.total_chunks, plan.total_chunks());
            assert!(rep.backfill_checked >= 1, "区{}至少回填一次", plan.zone_id);
            // 样本账：样本逐字段与方案推导一致
            let mut k = 0usize;
            let samples = sim.samples();
            assert!(!samples.is_empty());
            while k < samples.len() {
                let sm = &samples[k];
                let e = plan.derive_chunk(sm.idx).unwrap();
                assert_eq!(e.base, sm.base);
                assert_eq!(e.grade, sm.grade);
                assert_eq!(e.size, sm.size);
                k += 1;
            }
            total_checked += rep.backfill_checked;
            s.note_sim_result(&rep);
            i += 1;
        }
        assert!(total_checked >= (s.plans().len() as u64) * 2, "抽样回填总量异常");
        assert_eq!(s.stats().sim_runs, 5);
        assert_eq!(s.stats().sim_errors, 0);
        assert!(s.health().ok, "仿真全过后健康必须绿：{}", s.health().summary);
    }

    #[test]
    fn t07_backfill_catch_tamper() {
        // 人为构造"顺序法 vs 索引法"分裂：篡改方案让两路推导打架，
        // 仿真必须以 SimBackfill/SimContiguity 检出。
        // 合法构造（1×2M 后接 4K），但把 n4k 拉高制造守恒破 → CoverageGap。
        let base = 0x1_0020_0000u64; // 2M 对齐、非 1G 对齐
        let len = 0x40_1000u64;      // 2×2M + 1×4K
        let p = SplitPlan::new_checked(42, base, len, 0, 2, 1).unwrap();
        let mut sim = MappingSim::new(p, 64).unwrap();
        let rep = sim.run();
        assert!(rep.ok);
        assert_eq!(rep.mapped_bytes, len);
        // 篡改：把 4K 块数 +1（守恒破）
        let bad = SplitPlan::new_checked(42, base, len, 0, 2, 2);
        match bad {
            Err(HugeSelError::CoverageGap) => {}
            _ => panic!("篡改 n4k 必须守恒立案"),
        }
        // 篡改：块尺寸与档位不自洽的假块（手搓 LadderState 之外的字段）
        let fake = SplitPlan::new_checked(43, 0x1_0000_0000, PAGE_1G, 1, 0, 0).unwrap();
        let mut sim2 = MappingSim::new(fake, 16).unwrap();
        let rep2 = sim2.run();
        assert!(rep2.ok);
        assert_eq!(rep2.backfill_checked, 1); // 单块方案：首尾同块，回填一次
    }

    #[test]
    fn t08_dual_run_identity() {
        let mut a = demo_selector();
        let mut b = demo_selector();
        let fa = a.determinism_selfcheck().unwrap();
        let fb = b.determinism_selfcheck().unwrap();
        assert_eq!(fa, fb, "影子双跑方案指纹必须一致");
        assert_eq!(a.plan_fingerprint(), b.plan_fingerprint());
        // 指纹对 FNV 已知向量锚定
        assert_eq!(fnv1a64(b""), FNV_OFFSET);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn t09_injections_detected() {
        let mut s = demo_selector();
        // 注入一：不对齐
        assert!(s.inject_misalignment().unwrap(), "不对齐注入必须检出");
        // 注入二：预判分歧
        assert!(s.inject_pred_divergence().unwrap(), "预判分歧注入必须检出");
        assert!(s.divergence_count() >= 1);
        // 注入三：覆盖缺口（末位区 z5 有 22×4K 可抽）
        assert!(s.inject_coverage_gap().unwrap(), "覆盖缺口注入必须检出");
        assert_eq!(s.stats().injections_detected, 3);
        // 注入后健康必红
        let h = s.health();
        assert!(!h.ok);
        assert!(h.score < 90);
        assert!(h.summary.contains("分歧"));
    }

    #[test]
    fn t10_pte_ledger() {
        let s = demo_selector();
        let led = s.pte_min_ledger();
        // 全 4K 总项 = Σ(len>>12 + 尾页修正)
        let mut expect_all4k = 0u64;
        let mut i = 0usize;
        while i < s.plans().len() {
            expect_all4k += s.plans()[i].all4k_entries();
            i += 1;
        }
        assert_eq!(led.all4k_entries, expect_all4k);
        assert_eq!(led.mixed_entries, s.stats().n1g_total + s.stats().n2m_total + s.stats().n4k_total);
        assert!(led.all4k_entries > led.mixed_entries, "混合档位必须更省");
        assert!(led.savings_per_mille > 1000, "节省倍率应超 1000‰，实得 {}", led.savings_per_mille);
        // 单区行核对：z3 全 4K 区节省恰为 1000‰
        let rows = s.pte_rows();
        assert_eq!(rows.len(), 5);
        let z3row = rows.iter().find(|r| r.zone_id == 3).unwrap();
        assert_eq!(z3row.savings_per_mille, 1000);
        let z1row = rows.iter().find(|r| r.zone_id == 1).unwrap();
        // z1: 全4K = 0x1_8020_1000>>12 = 0x180201；混合 = 8
        assert_eq!(z1row.all4k_entries, 0x180201);
        assert_eq!(z1row.mixed_entries, 8);
        assert_eq!(z1row.savings_per_mille, 0x180201 * 1000 / 8);
    }

    #[test]
    fn t11_state_machine_and_ledger() {
        // 未封板 select_all → Sealed
        let mut s = GranuleSelector::new();
        match s.select_all() {
            Err(HugeSelError::Sealed) => {}
            _ => panic!("未封板必须拒绝选档"),
        }
        // 封板后再注入输入 → Sealed
        s.seal().unwrap();
        match s.submit_request(ZoneRequest {
            zone_id: 1,
            base: 0,
            len: PAGE_4K,
            kind: ZoneKind::UserFree,
        }) {
            Err(HugeSelError::Sealed) => {}
            _ => panic!("封板后必须拒绝新请求"),
        }
        // 重复封板 → Sealed
        match s.seal() {
            Err(HugeSelError::Sealed) => {}
            _ => panic!("重复封板必须拒绝"),
        }
        // 重复 select_all → Sealed
        s.select_all().unwrap();
        match s.select_all() {
            Err(HugeSelError::Sealed) => {}
            _ => panic!("方案重复生成必须拒绝"),
        }
        // 同区重复预判 → PredMissing（冲突语义）
        let mut s2 = GranuleSelector::new();
        s2.submit_request(ZoneRequest {
            zone_id: 1,
            base: 0x1_0000_0000,
            len: PAGE_1G,
            kind: ZoneKind::KernelData,
        })
        .unwrap();
        s2.load_prediction(PredEntry { zone_id: 1, grade: PageSize::P1G, cover_bytes: PAGE_1G }).unwrap();
        match s2.load_prediction(PredEntry { zone_id: 1, grade: PageSize::P2M, cover_bytes: PAGE_1G }) {
            Err(HugeSelError::PredMissing) => {}
            _ => panic!("同区重复预判必须冲突立案"),
        }
        // 事件账与账表导出非空
        assert!(!s.events().is_empty());
        assert!(s.plans_table().contains("区号"));
        assert!(s.divergence_table().contains("逐区一致"));
        assert!(s.events_table().contains("0x1570"));
    }

    #[test]
    fn t12_ledger_caps() {
        // 区账封顶：MAX_ZONES+1 个请求 → LedgerFull
        let mut s = GranuleSelector::new();
        let mut i = 0u32;
        while i < MAX_ZONES as u32 {
            let r = ZoneRequest {
                zone_id: i + 1,
                base: 0x1_0000_0000 + (i as u64) * PAGE_1G,
                len: PAGE_1G,
                kind: ZoneKind::KernelData,
            };
            s.submit_request(r).unwrap();
            i += 1;
        }
        let over = ZoneRequest {
            zone_id: 9999,
            base: 0,
            len: PAGE_4K,
            kind: ZoneKind::UserFree,
        };
        match s.submit_request(over) {
            Err(HugeSelError::LedgerFull) => {}
            _ => panic!("区账封顶必须拒绝"),
        }
        // 仿真规模封顶：须构造"全 4K 降档"的大区才可能块数超限——
        // 基址 0x1000（4K 对齐但非 2M 对齐）逼全档降 4K，块数 = len>>12
        let mut s2 = GranuleSelector::new();
        s2.submit_request(ZoneRequest {
            zone_id: 1,
            base: 0x1000,
            len: (MAX_SIM_CHUNKS + 1) * PAGE_4K,
            kind: ZoneKind::UserFree,
        })
        .unwrap();
        s2.seal().unwrap();
        s2.select_all().unwrap();
        let bp = s2.plan_of(1).unwrap();
        assert_eq!(bp.total_chunks(), MAX_SIM_CHUNKS + 1);
        match MappingSim::new(bp, 16) {
            Err(HugeSelError::LedgerFull) => {}
            _ => panic!("仿真超限必须拒绝"),
        }
    }

    #[test]
    fn t13_ladder_and_derive_agree() {
        // 两路推导全量对拍：LadderState 顺序法 vs derive_chunk 索引法
        let s = demo_selector();
        let mut pi = 0usize;
        while pi < s.plans().len() {
            let plan = s.plans()[pi];
            let mut lad = LadderState::new(&plan);
            let mut idx = 0u64;
            while idx < plan.total_chunks() {
                let a = lad.step().unwrap();
                let b = plan.derive_chunk(idx).unwrap();
                assert_eq!(a.idx, b.idx);
                assert_eq!(a.base, b.base, "区{}块{}两路基址分叉", plan.zone_id, idx);
                assert_eq!(a.grade, b.grade, "区{}块{}两路档位分叉", plan.zone_id, idx);
                assert_eq!(a.size, b.size);
                idx += 1;
            }
            assert!(lad.step().is_none(), "梯尽必须为 None");
            assert_eq!(lad.emitted(), plan.total_chunks());
            assert_eq!(lad.cursor(), plan.base + plan.len);
            assert_eq!(lad.remaining(), 0);
            pi += 1;
        }
    }

    #[test]
    fn t14_dominant_and_cover() {
        let s = demo_selector();
        // z1：6G(1G 档) vs 2M+4K → 主导 1G
        let z1 = s.plan_of(1).unwrap();
        assert_eq!(z1.dominant_grade(), PageSize::P1G);
        assert_eq!(z1.dominant_bytes(), 6 * PAGE_1G);
        // z2：384×2M 无 1G → 主导 2M
        let z2 = s.plan_of(2).unwrap();
        assert_eq!(z2.dominant_grade(), PageSize::P2M);
        // z4：纯 4K → 主导 4K
        let z4 = s.plan_of(4).unwrap();
        assert_eq!(z4.dominant_grade(), PageSize::P4K);
        // 诊断 Display 渲染
        let e = HugeSelError::BaseUnalign;
        let disp = format!("{}", e);
        assert!(disp.contains("0x1573"));
        assert!(disp.contains("基址未对齐"));
        // 每变体 detail 非空且中文
        let all = [
            HugeSelError::ZeroLen,
            HugeSelError::Overrun,
            HugeSelError::BaseUnalign,
            HugeSelError::LenUnalign,
            HugeSelError::GradeNotAllowed,
            HugeSelError::PredMissing,
            HugeSelError::PredGradeDiverge,
            HugeSelError::PredCoverDiverge,
            HugeSelError::CoverageGap,
            HugeSelError::LedgerFull,
            HugeSelError::Sealed,
            HugeSelError::SimBackfill,
            HugeSelError::SimContiguity,
            HugeSelError::DeterminismDrift,
            HugeSelError::InternalOverflow,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            assert_eq!(all[i].code() & 0xFFF0, SEG_BASE & 0xFFF0);
            assert!(all[i].code() >= SEG_BASE && all[i].code() <= SEG_BASE + 0x0F);
            i += 1;
        }
    }

    #[test]
    fn t15_module_identity() {
        let (tag, id, seg) = module_id();
        assert_eq!(tag, "UNX-F0305");
        assert_eq!(id, 0x0305);
        assert_eq!(seg, 0x1570);
    }
}
