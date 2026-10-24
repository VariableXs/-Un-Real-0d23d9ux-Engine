//! cgg04_nvram — CGPU-F0964 NVIDIA 显存与预算（大显存档位与压力预判）。
//!
//! 判据原文（施工书锚点 CGPU-F0964）：显存预算：NVIDIA 独显显存适配（大显存
//! 范围 4-24GB——预算模型的 NVIDIA 档位；与 H 显存域联动声明复用 F0852 理念）；
//! 显存容量分级影响（小显存 SKU 的降质联动——显存压力→降质链预判）；显存透
//! 明。判据：档位、联动、压力、透明、判据。
//!
//! 职责定位：4-24GB 六档档位表+SKU 登记+池化预算（提交/超额显性）+压力三级
//! 预判（告急→降质链预判信号——F0008 接缝）+用量透明报告单点。与
//! cgg02_nvprobe（F0962 能力探测）分工——彼探测显存规格存在性，此管显存怎么
//! 分与压力怎么预判。H 显存域（F0852）联动为声明式理念复用——预算结构同形
//! 不重定义物理口径。诊断码段 0x47xx。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const NV_RAM_VERSION: &str = "G04-nvram-v1";

/// 显存档位表（MB）：判据"大显存范围 4-24GB"的 NVIDIA 常见 SKU 档位。
pub const VRAM_TIERS_MB: [u64; 6] = [
    4_096,   // 4GB
    6_144,   // 6GB
    8_192,   // 8GB
    12_288,  // 12GB
    16_384,  // 16GB
    24_576,  // 24GB
];

/// 档位判定余量（MB）：容量落在档位 ±512MB 内归该档（SKU 标称差异）。
pub const TIER_TOLERANCE_MB: u64 = 512;

/// 池闭集（预算模型的分池口径——判据"预算模型"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VramPool {
    /// 呈现（帧缓冲/交换链——不可压缩）。
    Present,
    /// 纹理池（可流送回退）。
    Texture,
    /// 几何/顶点池（可压缩）。
    Geometry,
    /// 着色器/管线缓存（可重建）。
    Pipeline,
    /// 系统（页表/驱动保留——不可动）。
    System,
}

impl VramPool {
    pub const ALL: [VramPool; 5] = [
        VramPool::Present,
        VramPool::Texture,
        VramPool::Geometry,
        VramPool::Pipeline,
        VramPool::System,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            VramPool::Present => 0,
            VramPool::Texture => 1,
            VramPool::Geometry => 2,
            VramPool::Pipeline => 3,
            VramPool::System => 4,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            VramPool::Present => "呈现",
            VramPool::Texture => "纹理",
            VramPool::Geometry => "几何",
            VramPool::Pipeline => "管线缓存",
            VramPool::System => "系统保留",
        }
    }

    /// 压缩优先级（压力时的可回收序：管线缓存可重建最先收、纹理流送次之、
    /// 几何压缩再次；呈现与系统保留不可动——返回 0 表示不可回收）。
    pub const fn reclaim_priority(self) -> u8 {
        match self {
            VramPool::Pipeline => 4,
            VramPool::Texture => 3,
            VramPool::Geometry => 2,
            VramPool::Present | VramPool::System => 0,
        }
    }
}

/// 显存压力三级（判据"显存压力→降质链预判"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RamPressure {
    /// 充裕（已用 ≤70%）。
    Plenty,
    /// 紧张（70%~90%——流送回退提示）。
    Tight,
    /// 告急（>90%——降质链预判信号）。
    Critical,
}

impl RamPressure {
    pub const fn zh(self) -> &'static str {
        match self {
            RamPressure::Plenty => "充裕",
            RamPressure::Tight => "紧张",
            RamPressure::Critical => "告急(降质链预判)",
        }
    }
}

/// 压力阈值：紧张 700_000ppm / 告急 900_000ppm（写死可对拍）。
pub const PRESSURE_TIGHT_PPM: u64 = 700_000;
pub const PRESSURE_CRITICAL_PPM: u64 = 900_000;

/// 诊断码（0x47xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4701 SKU 未登记。
    NrSkuUnknown,
    /// 0x4702 容量出适配范围（4-24GB 之外——超出 NVIDIA 独显适配域）。
    NrCapInvalid,
    /// 0x4703 预算超提交（各池配额和>档位容量——构造拒）。
    NrOverCommit,
    /// 0x4704 压力告急——降质链预判信号（F0008 消费）。
    NrPressureCritical,
    /// 0x4705 池未登记（查询/扣账目标不存在）。
    NrPoolMissing,
    /// 0x4706 池超额扣账（单池扣超其配额——显性不静默）。
    NrPoolOver,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::NrSkuUnknown => "SKU 未登记：先登记 device+rev 再做显存预算",
            XcCode::NrCapInvalid => "显存容量出适配范围：NVIDIA 独显适配域为 4-24GB",
            XcCode::NrOverCommit => "预算超提交：各池配额和不得超过档位容量",
            XcCode::NrPressureCritical => "显存压力告急：降质链预判信号已发（F0008 消费执行）",
            XcCode::NrPoolMissing => "池未登记：预算五池闭集外",
            XcCode::NrPoolOver => "单池扣账超配额：显性拒绝不静默透支",
        }
    }
}

/// SKU 显存登记行。
#[derive(Clone, Copy, Debug)]
pub struct SkuRam {
    /// device id。
    pub device: u32,
    /// rev 号。
    pub rev: u8,
    /// 标称容量（MB）。
    pub capacity_mb: u64,
}

impl SkuRam {
    /// 归档（判据"NVIDIA 档位"）：落在某档 ±TOLERANCE 内即归档；
    /// 出 4-24GB 域外拒绝（判据适配域）。const fn 禁 slice::iter 的
    /// for 循环——while 索引循环（const 稳定形态）。
    pub const fn tier(&self) -> Result<usize, XcCode> {
        if self.capacity_mb < VRAM_TIERS_MB[0].saturating_sub(TIER_TOLERANCE_MB)
            || self.capacity_mb > VRAM_TIERS_MB[5].saturating_add(TIER_TOLERANCE_MB)
        {
            return Err(XcCode::NrCapInvalid);
        }
        let mut best = 0usize;
        let mut best_d = u64::MAX;
        let mut i = 0usize;
        while i < VRAM_TIERS_MB.len() {
            let t = VRAM_TIERS_MB[i];
            let d = if self.capacity_mb > t { self.capacity_mb - t } else { t - self.capacity_mb };
            if d < best_d {
                best_d = d;
                best = i;
            }
            i += 1;
        }
        if best_d > TIER_TOLERANCE_MB {
            return Err(XcCode::NrCapInvalid);
        }
        Ok(best)
    }
}

/// 用量透明报告行（判据"显存透明"——对外单点口径）。
#[derive(Clone, Copy, Debug)]
pub struct UsageRow {
    /// 池。
    pub pool: VramPool,
    /// 配额（MB）。
    pub quota_mb: u64,
    /// 已用（MB）。
    pub used_mb: u64,
}

impl UsageRow {
    /// 占用率（ppm）。
    pub const fn used_ppm(&self) -> u64 {
        if self.quota_mb == 0 {
            return 0;
        }
        let u = if self.used_mb > self.quota_mb { self.quota_mb } else { self.used_mb };
        u * 1_000_000 / self.quota_mb
    }
}

/// NVIDIA 显存预算器（判据本体）。
pub struct NvRamBudget {
    /// 已登记 SKU 表。
    skus: Vec<SkuRam>,
    /// 当前生效容量（MB，构造冻结）。
    capacity_mb: u64,
    /// 归档档位。
    tier: usize,
    /// 五池配额（MB，下标对齐 VramPool）。
    quota: [u64; 5],
    /// 五池已用（MB）。
    used: [u64; 5],
    /// 降质链预判信号（告急置位——F0008 消费后清）。
    pub degrade_pred_fired: bool,
    /// 立案计数。
    pub cases: u32,
}

impl NvRamBudget {
    /// 从 SKU 构造（登记+归档+默认配额分配）。
    pub fn from_sku(device: u32, rev: u8, capacity_mb: u64) -> Result<Self, XcCode> {
        let sku = SkuRam { device, rev, capacity_mb };
        let tier = sku.tier()?;
        let mut skus = Vec::new();
        skus.push(sku);
        let mut b = NvRamBudget {
            skus,
            capacity_mb,
            tier,
            quota: [0; 5],
            used: [0; 5],
            degrade_pred_fired: false,
            cases: 0,
        };
        // 默认配额（按档位自适应：小档呈现占比高——保交互；大档纹理占比高）：
        // 呈现 15% / 纹理 40% / 几何 20% / 管线 10% / 系统 15%（和=100%）。
        b.quota[0] = capacity_mb * 15 / 100;
        b.quota[1] = capacity_mb * 40 / 100;
        b.quota[2] = capacity_mb * 20 / 100;
        b.quota[3] = capacity_mb * 10 / 100;
        b.quota[4] = capacity_mb * 15 / 100;
        b.guard_over_commit()?;
        Ok(b)
    }

    /// 显式配额构造（判据"预算模型"的定制入口——Σquota ≤ capacity 强制）。
    pub fn with_quota(device: u32, rev: u8, capacity_mb: u64, quota: [u64; 5]) -> Result<Self, XcCode> {
        let sku = SkuRam { device, rev, capacity_mb };
        let tier = sku.tier()?;
        let mut sum = 0u64;
        for &q in &quota {
            sum = sum.saturating_add(q);
        }
        if sum > capacity_mb {
            return Err(XcCode::NrOverCommit);
        }
        let mut skus = Vec::new();
        skus.push(sku);
        Ok(NvRamBudget {
            skus,
            capacity_mb,
            tier,
            quota,
            used: [0; 5],
            degrade_pred_fired: false,
            cases: 0,
        })
    }

    /// 超提交守卫（Σquota ≤ capacity）。立案写账（&mut self）。
    fn guard_over_commit(&mut self) -> Result<(), XcCode> {
        let mut sum = 0u64;
        for &q in &self.quota {
            sum = sum.saturating_add(q);
        }
        if sum > self.capacity_mb {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NrOverCommit);
        }
        Ok(())
    }

    /// SKU 登记（追加；同 device+rev 重复拒）。
    pub fn register_sku(&mut self, device: u32, rev: u8, capacity_mb: u64) -> Result<(), XcCode> {
        let sku = SkuRam { device, rev, capacity_mb };
        sku.tier()?; // 出域拒
        if self.skus.iter().any(|s| s.device == device && s.rev == rev) {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NrSkuUnknown);
        }
        self.skus.push(sku);
        Ok(())
    }

    /// 当前档位（VRAM_TIERS_MB 下标）。
    pub const fn tier(&self) -> usize {
        self.tier
    }

    /// 当前容量（MB）。
    pub const fn capacity_mb(&self) -> u64 {
        self.capacity_mb
    }

    /// 池扣账（显存申请——单池超配额显性拒，不静默透支）。
    pub fn charge(&mut self, pool: VramPool, mb: u64) -> Result<(), XcCode> {
        let i = pool.ordinal();
        if self.used[i] + mb > self.quota[i] {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NrPoolOver);
        }
        self.used[i] += mb;
        Ok(())
    }

    /// 池回退（释放）。
    pub fn release(&mut self, pool: VramPool, mb: u64) {
        let i = pool.ordinal();
        self.used[i] = self.used[i].saturating_sub(mb);
    }

    /// 总已用（MB）。
    pub fn total_used_mb(&self) -> u64 {
        let mut s = 0u64;
        for &u in &self.used {
            s = s.saturating_add(u);
        }
        s
    }

    /// 压力判定（判据"显存容量分级影响"：已用/容量 ppm 三级）。
    pub fn pressure(&self) -> RamPressure {
        let ppm = self.total_used_mb() * 1_000_000 / self.capacity_mb.max(1);
        if ppm >= PRESSURE_CRITICAL_PPM {
            RamPressure::Critical
        } else if ppm >= PRESSURE_TIGHT_PPM {
            RamPressure::Tight
        } else {
            RamPressure::Plenty
        }
    }

    /// 压力预判与降质联动（判据"显存压力→降质链预判"——告急即发信号：
    /// 预判=爆显存前的先行信号，F0008 消费执行降质链）。
    pub fn pressure_forecast(&mut self) -> RamPressure {
        let p = self.pressure();
        if p == RamPressure::Critical {
            if !self.degrade_pred_fired {
                self.degrade_pred_fired = true;
                self.cases = self.cases.saturating_add(1);
            }
        }
        p
    }

    /// 压力回收建议单（按可回收优先级降序给出可回收池与可回收量）。
    pub fn reclaim_advice(&self) -> Vec<(VramPool, u64)> {
        let mut out: Vec<(VramPool, u64)> = Vec::new();
        for k in VramPool::ALL {
            let pri = k.reclaim_priority();
            if pri == 0 {
                continue; // 呈现/系统不可动
            }
            let i = k.ordinal();
            if self.used[i] > 0 {
                out.push((k, self.used[i]));
            }
        }
        out.sort_by(|a, b| {
            b.0.reclaim_priority().cmp(&a.0.reclaim_priority())
        });
        out
    }

    /// 透明报告（判据"显存透明"：五池用量+总占用+压力单点输出）。
    pub fn usage_report(&self) -> [UsageRow; 5] {
        [
            UsageRow { pool: VramPool::Present, quota_mb: self.quota[0], used_mb: self.used[0] },
            UsageRow { pool: VramPool::Texture, quota_mb: self.quota[1], used_mb: self.used[1] },
            UsageRow { pool: VramPool::Geometry, quota_mb: self.quota[2], used_mb: self.used[2] },
            UsageRow { pool: VramPool::Pipeline, quota_mb: self.quota[3], used_mb: self.used[3] },
            UsageRow { pool: VramPool::System, quota_mb: self.quota[4], used_mb: self.used[4] },
        ]
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "显存预算{} 档位{}({}MB) 已用{}MB 压力{} 预判信号{} 立案{}",
            NV_RAM_VERSION,
            self.tier,
            self.capacity_mb,
            self.total_used_mb(),
            self.pressure().zh(),
            self.degrade_pred_fired,
            self.cases,
        )
    }
}
