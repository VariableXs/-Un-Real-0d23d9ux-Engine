//! vck13_hetsec — CGPU-F1613 异构安全边界（模型三关+DMA 隔离+配额+供应链+隐私）。
//!
//! 判据原文（施工书锚点 CGPU-F1613）：安全：引擎安全（NPU 模型文件=不可信
//! 输入（模型解析安全（格式校验/大小上限/沙箱加载——模型安全规范；DMA 边
//! 界（引擎 DMA 访问范围限制（IOMMU/MMU 隔离（平台能力声明——DMA 隔离（越
//! 界防护；跨引擎攻击面（恶意任务占死引擎（超时+配额防护——配额复用；模
//! 型供应链（模型来源签名校验预留——供应链声明；隐私（NPU 推理数据（用户
//! 数据不落引擎持久区——数据生命周期声明；测试（模型 fuzz/DMA 隔离/配额/
//! 供应链/隐私五组）。判据：模型安全、DMA 隔离、配额、供应链、隐私、五组、
//! 判据。
//!
//! 职责定位：模型解析三关（NPU 模型文件=不可信输入：格式校验→大小上限→
//! 沙箱加载逐关放行，缺一关 0x8101 拒载——不可信输入没有"看起来无害"）+
//! DMA 隔离（引擎 DMA 访问范围限制：IOMMU/MMU 隔离边界+平台能力声明，越
//! 界访问 0x8102 拦截）+配额防护（恶意任务占死引擎：超时+配额双护栏——
//! 配额击穿 0x8103）+供应链声明（模型来源签名校验预留位显性在册——失签
//! 0x8104）+隐私生命周期（NPU 推理数据不落引擎持久区——落盘 0x8105）。诊
//! 断码段 0x8101~0x8105。


/// 模块版本。
pub const HET_SEC_VERSION: &str = "K13-hetsec-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x81 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x8101 模型解析失守（格式/大小/沙箱三关缺一）。
pub const XC_MODEL_PARSE_BREACH: u32 = 0x8101;
/// 0x8102 DMA 越界（引擎 DMA 访问越隔离边界）。
pub const XC_DMA_OOB: u32 = 0x8102;
/// 0x8103 配额击穿（恶意任务占死引擎）。
pub const XC_QUOTA_BREACH: u32 = 0x8103;
/// 0x8104 供应链失签（模型来源签名校验缺失）。
pub const XC_SUPPLY_CHAIN_MISS: u32 = 0x8104;
/// 0x8105 隐私破（推理数据落引擎持久区）。
pub const XC_PRIVACY_PERSIST: u32 = 0x8105;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 模型大小上限（字节——16MiB）。
pub const MODEL_SIZE_CAP: u32 = 16 * 1024 * 1024;
/// 沙箱魔数（模型头合法标识）。
pub const MODEL_SANDBOX_MAGIC: u32 = 0x4E4D_444C; // "NMDL"
/// DMA 隔离域数。
pub const DMA_DOMAIN_COUNT: usize = 4;
/// 每引擎任务配额（窗口内任务数上限）。
pub const TASK_QUOTA: u32 = 64;
/// 任务超时（µs）。
pub const TASK_TIMEOUT_US: u32 = 50_000;

// ---------------------------------------------------------------------------
// 一、模型解析三关（不可信输入）
// ---------------------------------------------------------------------------

/// 模型候选（待加载的不可信输入）。
#[derive(Clone, Copy, Debug)]
pub struct ModelCandidate {
    /// 头魔数。
    pub magic: u32,
    /// 大小（字节）。
    pub size: u32,
    /// 沙箱加载标记（加载器在沙箱内执行解析）。
    pub sandboxed: bool,
}

/// 模型解析守卫（判据"格式校验/大小上限/沙箱加载"三关）。
#[derive(Debug)]
pub struct ModelParseGuard {
    /// 已放行模型数。
    pub admitted: u32,
    /// 拒载事件数（0x8101）。
    pub rejected: u32,
}

impl ModelParseGuard {
    /// 新守卫。
    pub const fn new() -> ModelParseGuard {
        ModelParseGuard {
            admitted: 0,
            rejected: 0,
        }
    }

    /// 三关校验（逐关放行，缺一 0x8101）。
    pub fn admit(&mut self, m: &ModelCandidate) -> Result<(), u32> {
        // 关一：格式校验（魔数）。
        if m.magic != MODEL_SANDBOX_MAGIC {
            self.rejected += 1;
            return Err(XC_MODEL_PARSE_BREACH);
        }
        // 关二：大小上限。
        if m.size == 0 || m.size > MODEL_SIZE_CAP {
            self.rejected += 1;
            return Err(XC_MODEL_PARSE_BREACH);
        }
        // 关三：沙箱加载。
        if !m.sandboxed {
            self.rejected += 1;
            return Err(XC_MODEL_PARSE_BREACH);
        }
        self.admitted += 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、DMA 隔离（IOMMU 边界+越界防护）
// ---------------------------------------------------------------------------

/// DMA 隔离域（判据"IOMMU/MMU 隔离（平台能力声明"）。
#[derive(Clone, Copy, Debug)]
pub struct DmaDomain {
    /// 域序。
    pub id: usize,
    /// 基址（含）。
    pub base: u64,
    /// 上界（不含）。
    pub limit: u64,
    /// 平台能力声明在册（IOMMU 支持标记）。
    pub iommu_declared: bool,
}

/// DMA 隔离簿（判据"DMA 边界（引擎 DMA 访问范围限制"）。
#[derive(Debug)]
pub struct DmaIsolation {
    /// 四域表（NPU/GPU/DSP/CPU 各一域）。
    pub domains: [DmaDomain; DMA_DOMAIN_COUNT],
    /// 越界事件数（0x8102）。
    pub oob_events: u32,
}

impl DmaIsolation {
    /// 出厂：四域等分 4GiB 空间，IOMMU 声明在册。
    pub const fn new() -> DmaIsolation {
        DmaIsolation {
            domains: [
                DmaDomain { id: 0, base: 0x0000_0000, limit: 0x4000_0000, iommu_declared: true },
                DmaDomain { id: 1, base: 0x4000_0000, limit: 0x8000_0000, iommu_declared: true },
                DmaDomain { id: 2, base: 0x8000_0000, limit: 0xC000_0000, iommu_declared: true },
                DmaDomain { id: 3, base: 0xC000_0000, limit: 0x1_0000_0000, iommu_declared: true },
            ],
            oob_events: 0,
        }
    }

    /// DMA 访问裁决（域内放行/越界 0x8102）。
    pub fn access(&mut self, domain: usize, addr: u64, len: u64) -> Result<(), u32> {
        if domain >= DMA_DOMAIN_COUNT {
            self.oob_events += 1;
            return Err(XC_DMA_OOB);
        }
        let d = &self.domains[domain];
        // IOMMU 能力缺失=整域拒（平台没能力声明还开 DMA=裸奔）。
        if !d.iommu_declared {
            self.oob_events += 1;
            return Err(XC_DMA_OOB);
        }
        if addr < d.base || addr + len > d.limit || addr + len < addr {
            self.oob_events += 1;
            return Err(XC_DMA_OOB);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、配额防护（超时+配额）
// ---------------------------------------------------------------------------

/// 配额防护器（判据"恶意任务占死引擎（超时+配额防护——配额复用"）。
#[derive(Debug)]
pub struct QuotaGuard {
    /// 每引擎窗口任务计数（四引擎）。
    pub counts: [u32; 4],
    /// 击穿事件数（0x8103）。
    pub breach_events: u32,
}

impl QuotaGuard {
    /// 新防护器。
    pub const fn new() -> QuotaGuard {
        QuotaGuard {
            counts: [0; 4],
            breach_events: 0,
        }
    }

    /// 任务准入（配额满 0x8103；超时任务不计配额但记超时账——由调用方裁）。
    pub fn admit(&mut self, cls: usize) -> Result<(), u32> {
        if cls >= 4 {
            self.breach_events += 1;
            return Err(XC_QUOTA_BREACH);
        }
        if self.counts[cls] >= TASK_QUOTA {
            self.breach_events += 1;
            return Err(XC_QUOTA_BREACH);
        }
        self.counts[cls] += 1;
        Ok(())
    }

    /// 超时裁决（单任务超时→强制回收+配额回退一格）。
    pub fn timeout(&mut self, cls: usize) -> Result<(), u32> {
        if cls >= 4 || self.counts[cls] == 0 {
            return Err(XC_QUOTA_BREACH);
        }
        self.counts[cls] -= 1;
        Ok(())
    }

    /// 窗口重置（配额窗口滚动）。
    pub fn reset_window(&mut self) {
        self.counts = [0; 4];
    }
}

// ---------------------------------------------------------------------------
// 四、供应链声明 + 隐私生命周期
// ---------------------------------------------------------------------------

/// 供应链声明（判据"模型来源签名校验预留——供应链声明"）。
#[derive(Debug)]
pub struct SupplyChain {
    /// 签名校验预留位在册。
    pub sig_reserve_declared: bool,
    /// 已验签模型数。
    pub verified: u32,
    /// 失签事件数（0x8104）。
    pub miss_events: u32,
}

impl SupplyChain {
    /// 新声明（预留位出厂即在册）。
    pub const fn new() -> SupplyChain {
        SupplyChain {
            sig_reserve_declared: true,
            verified: 0,
            miss_events: 0,
        }
    }

    /// 模型验签（签名非零过；零签=失签 0x8104——预留位在册但校验不豁免）。
    pub fn verify(&mut self, sig: u64) -> Result<(), u32> {
        if !self.sig_reserve_declared || sig == 0 {
            self.miss_events += 1;
            return Err(XC_SUPPLY_CHAIN_MISS);
        }
        self.verified += 1;
        Ok(())
    }
}

/// 隐私生命周期声明（判据"NPU 推理数据（用户数据不落引擎持久区"）。
#[derive(Debug)]
pub struct PrivacyLifecycle {
    /// 生命周期声明在册（数据仅驻留易失区）。
    pub declared: bool,
    /// 落盘事件数（0x8105）。
    pub persist_events: u32,
}

impl PrivacyLifecycle {
    /// 新声明。
    pub const fn new() -> PrivacyLifecycle {
        PrivacyLifecycle {
            declared: true,
            persist_events: 0,
        }
    }

    /// 推理数据归宿裁决（落持久区 0x8105）。
    pub fn dispose(&mut self, to_persistent: bool) -> Result<(), u32> {
        if !self.declared || to_persistent {
            self.persist_events += 1;
            return Err(XC_PRIVACY_PERSIST);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、总引擎 + 五组自检
// ---------------------------------------------------------------------------

/// 异构安全边界总引擎。
#[derive(Debug)]
pub struct HetSecEngine {
    /// 模型解析守卫。
    pub model: ModelParseGuard,
    /// DMA 隔离簿。
    pub dma: DmaIsolation,
    /// 配额防护器。
    pub quota: QuotaGuard,
    /// 供应链声明。
    pub supply: SupplyChain,
    /// 隐私生命周期。
    pub privacy: PrivacyLifecycle,
}

impl HetSecEngine {
    /// 出厂。
    pub const fn new() -> HetSecEngine {
        HetSecEngine {
            model: ModelParseGuard::new(),
            dma: DmaIsolation::new(),
            quota: QuotaGuard::new(),
            supply: SupplyChain::new(),
            privacy: PrivacyLifecycle::new(),
        }
    }

    /// 组一：模型 fuzz——三关逐关拦截；全过放行。
    pub fn selfcheck_model(&mut self) -> Result<(), u32> {
        let mut e = HetSecEngine::new();
        // 坏魔数→0x8101。
        if e.model.admit(&ModelCandidate { magic: 0xDEAD_BEEF, size: 1024, sandboxed: true })
            != Err(XC_MODEL_PARSE_BREACH)
        {
            return Err(XC_MODEL_PARSE_BREACH);
        }
        // 超大小→0x8101。
        if e.model.admit(&ModelCandidate { magic: MODEL_SANDBOX_MAGIC, size: MODEL_SIZE_CAP + 1, sandboxed: true })
            != Err(XC_MODEL_PARSE_BREACH)
        {
            return Err(XC_MODEL_PARSE_BREACH);
        }
        // 零大小→0x8101。
        if e.model.admit(&ModelCandidate { magic: MODEL_SANDBOX_MAGIC, size: 0, sandboxed: true })
            != Err(XC_MODEL_PARSE_BREACH)
        {
            return Err(XC_MODEL_PARSE_BREACH);
        }
        // 非沙箱→0x8101。
        if e.model.admit(&ModelCandidate { magic: MODEL_SANDBOX_MAGIC, size: 1024, sandboxed: false })
            != Err(XC_MODEL_PARSE_BREACH)
        {
            return Err(XC_MODEL_PARSE_BREACH);
        }
        if e.model.rejected != 4 {
            return Err(XC_MODEL_PARSE_BREACH);
        }
        // 全过→放行。
        e.model.admit(&ModelCandidate { magic: MODEL_SANDBOX_MAGIC, size: 1024, sandboxed: true })?;
        if e.model.admitted != 1 {
            return Err(XC_MODEL_PARSE_BREACH);
        }
        Ok(())
    }

    /// 组二：DMA 隔离——域内放行；越界拦；IOMMU 缺失整域拒。
    pub fn selfcheck_dma(&mut self) -> Result<(), u32> {
        let mut e = HetSecEngine::new();
        // 域 0 内访问（0x1000+len 0x100 ≤ 0x4000_0000）→过。
        e.dma.access(0, 0x1000, 0x100)?;
        // 越上界→0x8102。
        if e.dma.access(0, 0x3FFF_FFFF, 0x100) != Err(XC_DMA_OOB) {
            return Err(XC_DMA_OOB);
        }
        // 越下界→0x8102。
        if e.dma.access(1, 0x1000, 0x100) != Err(XC_DMA_OOB) {
            return Err(XC_DMA_OOB);
        }
        // 越界域号→0x8102。
        if e.dma.access(9, 0x1000, 0x100) != Err(XC_DMA_OOB) {
            return Err(XC_DMA_OOB);
        }
        // IOMMU 缺失整域拒。
        e.dma.domains[2].iommu_declared = false;
        if e.dma.access(2, 0x8000_1000, 0x100) != Err(XC_DMA_OOB) {
            return Err(XC_DMA_OOB);
        }
        if e.dma.oob_events != 4 {
            return Err(XC_DMA_OOB);
        }
        Ok(())
    }

    /// 组三：配额——配额满击穿；超时回收；窗口重置。
    pub fn selfcheck_quota(&mut self) -> Result<(), u32> {
        let mut e = HetSecEngine::new();
        // 灌满 NPU（cls=1）配额。
        let mut i = 0;
        while i < TASK_QUOTA {
            e.quota.admit(1)?;
            i += 1;
        }
        // 第 65 个→击穿 0x8103。
        if e.quota.admit(1) != Err(XC_QUOTA_BREACH) {
            return Err(XC_QUOTA_BREACH);
        }
        // 超时回收一格→再准入过。
        e.quota.timeout(1)?;
        e.quota.admit(1)?;
        // 窗口重置→全空。
        e.quota.reset_window();
        if e.quota.counts[1] != 0 {
            return Err(XC_QUOTA_BREACH);
        }
        // 越界引擎号→击穿。
        if e.quota.admit(9) != Err(XC_QUOTA_BREACH) {
            return Err(XC_QUOTA_BREACH);
        }
        Ok(())
    }

    /// 组四：供应链+隐私——零签拒；落持久区拒。
    pub fn selfcheck_supply_privacy(&mut self) -> Result<(), u32> {
        let mut e = HetSecEngine::new();
        // 零签→0x8104。
        if e.supply.verify(0) != Err(XC_SUPPLY_CHAIN_MISS) {
            return Err(XC_SUPPLY_CHAIN_MISS);
        }
        if e.supply.miss_events != 1 {
            return Err(XC_SUPPLY_CHAIN_MISS);
        }
        // 有签→过。
        e.supply.verify(0xDEAD_C0DE_DEAD_C0DE)?;
        if e.supply.verified != 1 {
            return Err(XC_SUPPLY_CHAIN_MISS);
        }
        // 推理数据落持久区→0x8105。
        if e.privacy.dispose(true) != Err(XC_PRIVACY_PERSIST) {
            return Err(XC_PRIVACY_PERSIST);
        }
        // 易失区释放→过。
        e.privacy.dispose(false)?;
        if e.privacy.persist_events != 1 {
            return Err(XC_PRIVACY_PERSIST);
        }
        Ok(())
    }

    /// 组五：判据——端到端：模型+DMA+配额+供应链+隐私五件全链绿。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = HetSecEngine::new();
        e.model.admit(&ModelCandidate { magic: MODEL_SANDBOX_MAGIC, size: 4096, sandboxed: true })?;
        e.dma.access(1, 0x4000_0000, 0x1000)?;
        e.quota.admit(1)?;
        e.supply.verify(0x1234_5678_9ABC_DEF0)?;
        e.privacy.dispose(false)?;
        if e.model.rejected != 0
            || e.dma.oob_events != 0
            || e.quota.breach_events != 0
            || e.supply.miss_events != 0
            || e.privacy.persist_events != 0
        {
            return Err(XC_MODEL_PARSE_BREACH);
        }
        Ok(())
    }

    /// 全量自检入口（五组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_model()?;
        self.selfcheck_dma()?;
        self.selfcheck_quota()?;
        self.selfcheck_supply_privacy()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8100..0x8200 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_MODEL_PARSE_BREACH,
            XC_DMA_OOB,
            XC_QUOTA_BREACH,
            XC_SUPPLY_CHAIN_MISS,
            XC_PRIVACY_PERSIST,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8100 || codes[i] >= 0x8200 {
                return Err(XC_MODEL_PARSE_BREACH);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_MODEL_PARSE_BREACH);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
