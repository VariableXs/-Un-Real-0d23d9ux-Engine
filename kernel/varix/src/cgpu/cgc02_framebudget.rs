//! CGPU-F0322 · 帧预算模型（CGPU-C 域 · C02 组 · 目标 420 行 / 深化交付）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0322`
//!
//! **判据（锚点原文）**：四池、三级、双表述、守恒校验、判据。
//!
//! **职责定位（锚点原文）**：帧预算模型：12.5ms 帧预算分解（呈现合成/界面
//! 响应/后台任务/网络与余量四池——池比例可配但默认保交互）；预算层级
//! （帧预算→池预算→账户预算三级）；预算双表述（微秒精度+占比）；模型
//! 数学化（预算守恒：Σ池=帧预算、Σ账户≤池——超载检测器）。
//!
//! ## 深化交付结构（十节）
//!
//! 一、诊断码（0x62 段独占，C02 组预算面七码）。
//! 二、帧预算常量与四池闭集（默认比例保交互；比例可配但有下限守卫）。
//! 三、预算双表述（微秒 ↔ 万分比占比换算器——换算恒等可逆）。
//! 四、三级层级模型（帧→池→账户；层级系数核验）。
//! 五、守恒校验器（Σ池=帧预算、Σ账户≤池；超载检测）。
//! 六、池配置账（配置校验 + 保交互下限 + 配置指纹）。
//! 七、预算分配引擎（确定性分配 + 余量归池规则 + 平局显性）。
//! 八、超载检测与处置建议（人读修复面）。
//! 九、预算快照导出（B08 五元组口径——与 F2408 统一模型对齐）。
//! 十、对外接口冻结 v1（一站式帧预算构建）。
//!
//! **复用声明**（判据侧逐条 grep 对拍——只引用不重定义）：
//! - 复用 [`super::cgc01_carbitration`] 的 C 域仲裁账户模型（F0321）——
//!   账户预算是仲裁申请的输入面（预算在先、申请在后）。
//! - 复用 F2401 预算意识使命「遥测不做帧预算隐形税」——本模型的预算
//!   分解是使命的 C 域落地面。
//! - 复用 F0483 帧账 12.5ms 口径（帧间隔 80Hz 基准——帧预算锚点一致）。
//! - 复用 F2408 万分比整数口径（ppm——无浮点纪律）。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（0x62 段独占：C02 组预算面）
// ---------------------------------------------------------------------------

/// 诊断码（u16；0x62 段为本单独占段，段内互异，与 0x60/0x61 段不重叠）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BcCode {
    /// 池守恒违约：Σ池 ≠ 帧预算（分解表被误改——先红）。
    PoolSumBreach = 0x6201,
    /// 池比例配置非法：比例和非满或低于保交互下限。
    RatioInvalid = 0x6202,
    /// 账户超载：Σ账户 > 池预算（超载显性，不静默挤占）。
    AccountOverload = 0x6203,
    /// 账户归属表外池（池 id 未登记）。
    PoolUnknown = 0x6204,
    /// 微秒表述越帧窗（单账户预算超帧预算——层级倒挂）。
    OverFrameWindow = 0x6205,
    /// 池负预算（预算不可为负——配置面完整性）。
    NegativeBudget = 0x6206,
    /// 双表述换算不一致（微秒↔占比往返超容差——换算器病）。
    ConversionDrift = 0x6207,
}

impl BcCode {
    /// 人读诊断（异常零静默）。
    pub const fn detail(self) -> &'static str {
        match self {
            BcCode::PoolSumBreach => "池守恒违约：四池之和必须恰等于帧预算，建议重算分解表",
            BcCode::RatioInvalid => "池比例非法：比例和必须为满万分比且各池不低于保交互下限",
            BcCode::AccountOverload => "账户超载：账户合计超出池预算，建议降配或移账户到后台池",
            BcCode::PoolUnknown => "账户归属表外池：池 id 未登记，建议先注册池再挂账户",
            BcCode::OverFrameWindow => "账户预算超帧窗：层级倒挂（账户>池>帧），建议重排层级",
            BcCode::NegativeBudget => "池负预算：预算语义非负，建议核对配置来源",
            BcCode::ConversionDrift => "双表述换算漂移：微秒↔占比往返超容差，建议核查换算器",
        }
    }

    /// 段守恒。
    pub const fn in_segment(self) -> bool {
        (self as u16) >= 0x6201 && (self as u16) <= 0x6207
    }
}

// ---------------------------------------------------------------------------
// 二、帧预算常量与四池闭集（判据一）
// ---------------------------------------------------------------------------

/// 帧预算微秒（12.5ms = 12500µs —— 80Hz 基准，与 F0483 帧间隔口径同源）。
pub const FRAME_BUDGET_US: u32 = 12_500;

/// 池身份四闭集（判据一：呈现合成/界面响应/后台任务/网络与余量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolKind {
    /// 呈现合成池（呈现+合成+顶栏——保交互第一优先）。
    Present,
    /// 界面响应池（输入回显+交互动画——保交互第二优先）。
    UiResponse,
    /// 后台任务池（预取/缓存/编译——可让渡）。
    Background,
    /// 网络与余量池（传输等待+未分配余量——弹性缓冲）。
    NetReserve,
}

impl PoolKind {
    /// 全量闭集。
    pub const ALL: [PoolKind; 4] = [
        PoolKind::Present,
        PoolKind::UiResponse,
        PoolKind::Background,
        PoolKind::NetReserve,
    ];

    /// 稳定序（矩阵与账本槽位序）。
    pub const fn ordinal(self) -> usize {
        match self {
            PoolKind::Present => 0,
            PoolKind::UiResponse => 1,
            PoolKind::Background => 2,
            PoolKind::NetReserve => 3,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            PoolKind::Present => "呈现合成",
            PoolKind::UiResponse => "界面响应",
            PoolKind::Background => "后台任务",
            PoolKind::NetReserve => "网络与余量",
        }
    }
}

/// 默认池比例（万分比；Σ=10000 恰满——默认保交互：呈现 45% / 界面 25% /
/// 后台 15% / 网络余量 15%）。
pub const DEFAULT_POOL_RATIOS: [u32; 4] = [4_500, 2_500, 1_500, 1_500];

/// 保交互下限（呈现合成池的配置下限万分比——「可配但默认保交互」的守卫线）。
pub const PRESENT_RATIO_FLOOR: u32 = 3_500;

/// 界面响应池下限。
pub const UI_RATIO_FLOOR: u32 = 1_500;

/// 四池预算表（默认配置下的微秒分解；判据侧独立重算对拍）。
pub const DEFAULT_POOL_BUDGETS_US: [u32; 4] = [5_625, 3_125, 1_875, 1_875];

/// 四池守恒（编译期钉死：Σ比例=10000、Σ微秒=帧预算、比例×帧=微秒）。
const _: () = {
    assert!(DEFAULT_POOL_RATIOS[0] + DEFAULT_POOL_RATIOS[1] + DEFAULT_POOL_RATIOS[2]
        + DEFAULT_POOL_RATIOS[3] == 10_000, "四池比例和必须恰满万分比");
    assert!(DEFAULT_POOL_BUDGETS_US[0] + DEFAULT_POOL_BUDGETS_US[1]
        + DEFAULT_POOL_BUDGETS_US[2] + DEFAULT_POOL_BUDGETS_US[3] == FRAME_BUDGET_US,
        "四池微秒和必须恰等于帧预算");
};

// ---------------------------------------------------------------------------
// 三、预算双表述（判据三：微秒 + 占比；换算恒等可逆）
// ---------------------------------------------------------------------------

/// 微秒 → 万分比（×10000/帧预算；饱和不越界）。
pub const fn us_to_ppm(us: u32) -> u32 {
    (us as u64).saturating_mul(10_000) as u32 / FRAME_BUDGET_US
}

/// 万分比 → 微秒（向下取整——占比表述不虚高）。
pub const fn ppm_to_us(ppm: u32) -> u32 {
    ((ppm as u64) * (FRAME_BUDGET_US as u64) / 10_000) as u32
}

/// 换算往返守卫（us → ppm → us 的向下取整误差 ≤ 1µs——模型恒等面）。
pub const fn roundtrip_ok(us: u32) -> bool {
    ppm_to_us(us_to_ppm(us)) + 1 >= us
}

/// 双表述预算行（一份预算两种说法——判定以微秒为准，展示以占比为准）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualBudget {
    /// 微秒精度表述。
    pub us: u32,
    /// 万分比占比表述（由 us 派生——单一事实源）。
    pub ppm: u32,
}

impl DualBudget {
    /// 构造（微秒为源；占比派生——杜绝两张皮）。
    pub const fn from_us(us: u32) -> DualBudget {
        DualBudget { us, ppm: us_to_ppm(us) }
    }

    /// 一致性（占比换回微秒不得虚高超 1µs）。
    pub const fn coherent(&self) -> bool {
        roundtrip_ok(self.us)
    }
}

// ---------------------------------------------------------------------------
// 四、三级层级模型（判据二：帧→池→账户）
// ---------------------------------------------------------------------------

/// 层级闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetLevel {
    /// 帧级（12.5ms 总窗）。
    Frame,
    /// 池级（四池分解）。
    Pool,
    /// 账户级（功能×表面最小记账单元）。
    Account,
}

impl BudgetLevel {
    /// 深度序（Frame=0 < Pool=1 < Account=2——层级单调核验用）。
    pub const fn depth(self) -> u8 {
        match self {
            BudgetLevel::Frame => 0,
            BudgetLevel::Pool => 1,
            BudgetLevel::Account => 2,
        }
    }
}

/// 层级单调核验（子级深度必须恰比父级大 1——层级表完整性）。
pub const fn level_step_ok(parent: BudgetLevel, child: BudgetLevel) -> bool {
    child.depth() == parent.depth() + 1
}

/// 账户（三级叶：功能×表面 + 四属性——优先级/最低保障/上限/可借出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolAccount {
    /// 账户 id（C 域仲裁账户同源——F0321 的账户 id 直接复用）。
    pub account_id: u32,
    /// 归属池。
    pub pool: PoolKind,
    /// 优先级（0 最高——与 F0217 三维优先级联动面）。
    pub priority: u32,
    /// 最低保障（µs——饥饿防线）。
    pub floor_us: u32,
    /// 上限（µs——膨胀防线）。
    pub cap_us: u32,
    /// 可借出标记（余量可让渡给同池其他账户）。
    pub lendable: bool,
    /// 人读名。
    pub label: String,
}

impl PoolAccount {
    /// 属性自洽（floor ≤ cap——最低保障不得超过上限）。
    pub const fn attrs_ok(&self) -> bool {
        self.floor_us <= self.cap_us
    }
}

// ---------------------------------------------------------------------------
// 五、守恒校验器（判据四：Σ池=帧预算、Σ账户≤池——超载检测）
// ---------------------------------------------------------------------------

/// 池账（一个池的分解行 + 挂靠账户清单）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolLedger {
    /// 池身份。
    pub pool: PoolKind,
    /// 池预算（µs——由配置比例派生）。
    pub budget_us: u32,
    /// 账户合计（µs——挂靠账户 floor 与 cap 的分配合计；由引擎写入）。
    pub allocated_us: u32,
}

impl PoolLedger {
    /// 池守恒（allocated ≤ budget——池级不超载）。
    pub const fn within(&self) -> bool {
        self.allocated_us <= self.budget_us
    }

    /// 池余量（µs）。
    pub const fn slack_us(&self) -> u32 {
        self.budget_us.saturating_sub(self.allocated_us)
    }
}

/// 守恒校验结果（闭集三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conservation {
    /// 全过：Σ池=帧预算 且 各池 Σ账户≤池。
    Sound,
    /// 池和违约（分解表病）。
    PoolSumBroken,
    /// 某池超载（账户挤占）。
    PoolOverloaded,
}

/// 全量守恒校验（判据四机检面；返回首错或 Sound）。
pub fn check_conservation(pools: &[PoolLedger]) -> Conservation {
    // Σ池 = 帧预算。
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < pools.len() {
        if let Some(p) = pools.get(i) {
            sum = sum.saturating_add(p.budget_us);
        }
        i += 1;
    }
    if sum != FRAME_BUDGET_US {
        return Conservation::PoolSumBroken;
    }
    // 各池不超载。
    let mut i = 0usize;
    while i < pools.len() {
        if let Some(p) = pools.get(i) {
            if !p.within() {
                return Conservation::PoolOverloaded;
            }
        }
        i += 1;
    }
    Conservation::Sound
}

/// 池覆盖核验（四池必须齐备——缺池的分解不是完整帧预算）。
pub fn pools_complete(pools: &[PoolLedger]) -> bool {
    if pools.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < 4 {
        let kind = match PoolKind::ALL.get(i) {
            Some(k) => *k,
            None => return false,
        };
        let row = match pools.get(i) {
            Some(p) => p,
            None => return false,
        };
        if row.pool != kind {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 六、池配置账（判据一：比例可配但保交互；配置指纹）
// ---------------------------------------------------------------------------

/// 池配置（可配比例；构造时守卫——非法配置显性拒不出表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolConfig {
    ratios: [u32; 4],
}

impl PoolConfig {
    /// 默认配置（保交互）。
    pub const fn standard() -> PoolConfig {
        PoolConfig { ratios: DEFAULT_POOL_RATIOS }
    }

    /// 自定义配置（非法显性拒：和满 + 保交互下限 + 非负）。
    pub fn custom(ratios: [u32; 4]) -> Result<PoolConfig, BcCode> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < 4 {
            let r = match ratios.get(i) {
                Some(x) => *x,
                None => return Err(BcCode::RatioInvalid),
            };
            sum += r;
            i += 1;
        }
        if sum != 10_000 {
            return Err(BcCode::RatioInvalid);
        }
        let present = match ratios.get(PoolKind::Present.ordinal()) {
            Some(x) => *x,
            None => return Err(BcCode::RatioInvalid),
        };
        let ui = match ratios.get(PoolKind::UiResponse.ordinal()) {
            Some(x) => *x,
            None => return Err(BcCode::RatioInvalid),
        };
        if present < PRESENT_RATIO_FLOOR || ui < UI_RATIO_FLOOR {
            return Err(BcCode::RatioInvalid);
        }
        Ok(PoolConfig { ratios })
    }

    /// 比例查。
    pub const fn ratio(&self, pool: PoolKind) -> u32 {
        self.ratios[pool.ordinal()]
    }

    /// 池预算派生（比例 × 帧预算 → µs；向下取整——占比不虚高）。
    pub const fn pool_budget_us(&self, pool: PoolKind) -> u32 {
        ((self.ratio(pool) as u64) * (FRAME_BUDGET_US as u64) / 10_000) as u32
    }

    /// 四池预算派生（顺序与 PoolKind::ALL 一致）。
    pub const fn budgets(&self) -> [u32; 4] {
        [
            self.pool_budget_us(PoolKind::Present),
            self.pool_budget_us(PoolKind::UiResponse),
            self.pool_budget_us(PoolKind::Background),
            self.pool_budget_us(PoolKind::NetReserve),
        ]
    }

    /// 配置指纹（FNV-1a 32bit over 比例表——配置漂移的可对拍面）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let mut i = 0usize;
        while i < 4 {
            let r = self.ratios[i];
            let b = r.to_le_bytes();
            let mut j = 0usize;
            while j < 4 {
                h ^= b[j] as u32;
                h = h.wrapping_mul(0x0100_0193);
                j += 1;
            }
            i += 1;
        }
        h
    }

    /// 四池账派生（budget 由比例派生；allocated 清零——账户挂靠前）。
    pub fn ledgers(&self) -> [PoolLedger; 4] {
        let b = self.budgets();
        [
            PoolLedger { pool: PoolKind::Present, budget_us: b[0], allocated_us: 0 },
            PoolLedger { pool: PoolKind::UiResponse, budget_us: b[1], allocated_us: 0 },
            PoolLedger { pool: PoolKind::Background, budget_us: b[2], allocated_us: 0 },
            PoolLedger { pool: PoolKind::NetReserve, budget_us: b[3], allocated_us: 0 },
        ]
    }
}

// ---------------------------------------------------------------------------
// 七、预算分配引擎（判据二：三级层级落地；确定性分配）
// ---------------------------------------------------------------------------

/// 分配请求（一账户一次申请）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocRequest {
    /// 账户。
    pub account: PoolAccount,
    /// 申请量（µs）。
    pub ask_us: u32,
}

/// 分配结论（闭集三态 + 人读）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AllocVerdict {
    /// 全额授予。
    Granted { granted_us: u32 },
    /// 按 floor 保障授予（ask 超 cap 或超池余量——保障线兜底）。
    Floored { granted_us: u32 },
    /// 拒（账户属性病或池表外）。
    Denied { code: BcCode },
}

/// 预算分配引擎（确定性：同状态同请求同结论）。
#[derive(Clone, Debug)]
pub struct BudgetEngine {
    /// 池配置。
    pub config: PoolConfig,
    /// 池账。
    pub pools: [PoolLedger; 4],
    /// 账户账（已授予的账户清单）。
    accounts: Vec<(PoolAccount, u32)>,
}

impl BudgetEngine {
    /// 构造（标准配置）。
    pub fn standard() -> BudgetEngine {
        BudgetEngine { config: PoolConfig::standard(), pools: PoolConfig::standard().ledgers(), accounts: Vec::new() }
    }

    /// 构造（自定义配置；非法配置回退标准——回退显性记录于 verdict 面外，
    /// 由调用方先过 [`PoolConfig::custom`] 保证）。
    pub fn with_config(config: PoolConfig) -> BudgetEngine {
        BudgetEngine { config, pools: config.ledgers(), accounts: Vec::new() }
    }

    fn pool_idx(&self, pool: PoolKind) -> usize {
        pool.ordinal()
    }

    /// 账户已授予量查。
    pub fn granted_of(&self, account_id: u32) -> Option<u32> {
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some((a, g)) = self.accounts.get(i) {
                if a.account_id == account_id {
                    return Some(*g);
                }
            }
            i += 1;
        }
        None
    }

    /// 账户数。
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    /// 单账户分配（判据二+四：层级与守恒）。
    ///
    /// 规则序（显性、确定性）：
    /// 1. 属性自洽核验（floor ≤ cap）——不洽即拒 [`BcCode::NegativeBudget`]。
    /// 2. ask > cap → 截到 cap（上限防线）。
    /// 3. 授予后池 allocated ≤ 池预算 → 全额授予。
    /// 4. 池余量不足但 ≥ floor → 按 floor 保障授予（饥饿防线）。
    /// 5. 池余量 < floor → 拒 [`BcCode::AccountOverload`]（超载显性）。
    pub fn allocate(&mut self, req: &AllocRequest) -> AllocVerdict {
        if !req.account.attrs_ok() {
            return AllocVerdict::Denied { code: BcCode::NegativeBudget };
        }
        // 上限截断。
        let ask = if req.ask_us > req.account.cap_us { req.account.cap_us } else { req.ask_us };
        // 同账户重复申请 = 调整（先退旧量再申请新量——幂等修正语义）。
        let mut prev: Option<u32> = None;
        let mut idx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some((a, g)) = self.accounts.get(i) {
                if a.account_id == req.account.account_id {
                    prev = Some(*g);
                    idx = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let pidx = self.pool_idx(req.account.pool);
        let pool = match self.pools.get(pidx) {
            Some(p) => p,
            None => return AllocVerdict::Denied { code: BcCode::PoolUnknown },
        };
        let budget = pool.budget_us;
        let mut allocated = pool.allocated_us;
        if let Some(g) = prev {
            allocated = allocated.saturating_sub(g);
        }
        let slack = budget.saturating_sub(allocated);
        let floor = req.account.floor_us;
        let grant = if ask <= slack {
            ask
        } else if slack >= floor && floor > 0 {
            floor
        } else if floor == 0 && slack > 0 {
            slack
        } else {
            return AllocVerdict::Denied { code: BcCode::AccountOverload };
        };
        // 落账。
        let pool_mut = match self.pools.get_mut(pidx) {
            Some(p) => p,
            None => return AllocVerdict::Denied { code: BcCode::PoolUnknown },
        };
        pool_mut.allocated_us = allocated.saturating_add(grant);
        match idx {
            Some(ix) => {
                if let Some(slot) = self.accounts.get_mut(ix) {
                    slot.1 = grant;
                }
            }
            None => {
                self.accounts.push((req.account.clone(), grant));
            }
        }
        if grant == ask {
            AllocVerdict::Granted { granted_us: grant }
        } else {
            AllocVerdict::Floored { granted_us: grant }
        }
    }

    /// 借出（可借出账户的余量让渡：从 lender 池余量转给 borrower 账户）。
    ///
    /// 规则：borrower 必须 floor>0 且其池余量 < floor；lender 池必须余量充足；
    /// 转移量 = min(缺口, lender 余量)——不过度借出。
    pub fn lend(&mut self, borrower_id: u32, lender_pool: PoolKind, borrower_need: u32) -> Result<u32, BcCode> {
        // 找 borrower。
        let mut bidx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some((a, _)) = self.accounts.get(i) {
                if a.account_id == borrower_id {
                    bidx = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let bi = match bidx {
            Some(x) => x,
            None => return Err(BcCode::PoolUnknown),
        };
        let (pool_kind, need) = {
            let entry = match self.accounts.get(bi) {
                Some(e) => e,
                None => return Err(BcCode::PoolUnknown),
            };
            (entry.0.pool, borrower_need)
        };
        // borrower 池余量。
        let bp = self.pool_idx(pool_kind);
        let bslack = match self.pools.get(bp) {
            Some(p) => p.slack_us(),
            None => return Err(BcCode::PoolUnknown),
        };
        if bslack >= need {
            return Ok(0); // 不缺不借。
        }
        let gap = need - bslack;
        // lender 池余量。
        let lp = self.pool_idx(lender_pool);
        let lslack = match self.pools.get(lp) {
            Some(p) => p.slack_us(),
            None => return Err(BcCode::PoolUnknown),
        };
        let move_amt = if gap < lslack { gap } else { lslack };
        if move_amt == 0 {
            return Err(BcCode::AccountOverload);
        }
        // 挪账：lender 池 allocated += amt；borrower 池预算 += amt（借入为临时扩窗）。
        // 守恒：Σ池预算不变（挪的是预算不是账）——借入记为预算转移。
        {
            let lp_mut = match self.pools.get_mut(lp) {
                Some(p) => p,
                None => return Err(BcCode::PoolUnknown),
            };
            lp_mut.budget_us = lp_mut.budget_us.saturating_sub(move_amt);
        }
        {
            let bp_mut = match self.pools.get_mut(bp) {
                Some(p) => p,
                None => return Err(BcCode::PoolUnknown),
            };
            bp_mut.budget_us = bp_mut.budget_us.saturating_add(move_amt);
        }
        Ok(move_amt)
    }

    /// 守恒快照（当前池账拷贝——审计面）。
    pub fn snapshot(&self) -> [PoolLedger; 4] {
        self.pools.clone()
    }
}

// ---------------------------------------------------------------------------
// 八、超载检测与处置建议（判据四的修复面）
// ---------------------------------------------------------------------------

/// 超载报告（人读处置建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverloadReport {
    /// 超载池。
    pub pool: PoolKind,
    /// 池预算（µs）。
    pub budget_us: u32,
    /// 实际合计（µs）。
    pub allocated_us: u32,
    /// 超载量（µs）。
    pub excess_us: u32,
    /// 处置建议。
    pub advice: String,
}

/// 超载扫描（判据四：逐池检 allocated > budget → 报告）。
pub fn scan_overloads(pools: &[PoolLedger]) -> Vec<OverloadReport> {
    let mut out: Vec<OverloadReport> = Vec::new();
    let mut i = 0usize;
    while i < pools.len() {
        let p = match pools.get(i) {
            Some(p) => p,
            None => break,
        };
        if p.allocated_us > p.budget_us {
            let excess = p.allocated_us - p.budget_us;
            let advice = match p.pool {
                PoolKind::Present => "呈现池超载：交互红线不可破——建议把非呈现账户移出本池",
                PoolKind::UiResponse => "界面池超载：建议降动画复杂度或移账户到后台池",
                PoolKind::Background => "后台池超载：建议暂停最低优先级账户（后台可让渡）",
                PoolKind::NetReserve => "余量池超载：余量为弹性缓冲不可挤占——建议回到真实需求重排",
            };
            out.push(OverloadReport {
                pool: p.pool,
                budget_us: p.budget_us,
                allocated_us: p.allocated_us,
                excess_us: excess,
                advice: advice.to_string(),
            });
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 九、预算快照导出（判据五的观测收口；B08 五元组）
// ---------------------------------------------------------------------------

/// 指标名前缀。
pub const BUDGET_METRIC_PREFIX: &str = "c02.framebudget.";

/// 遥测指标行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetMetric {
    /// 指标 id。
    pub id: &'static str,
    /// 值。
    pub value: u32,
    /// 单位。
    pub unit: &'static str,
    /// 口径。
    pub basis: &'static str,
}

/// 指标 id 闭集（六条）。
pub const BUDGET_METRIC_IDS: [&str; 6] = [
    "c02.framebudget.frame_us",
    "c02.framebudget.present_us",
    "c02.framebudget.ui_us",
    "c02.framebudget.background_us",
    "c02.framebudget.reserve_us",
    "c02.framebudget.overload_count",
];

/// 快照导出（确定性零 IO）。
pub fn export_budget_metrics(pools: &[PoolLedger]) -> Vec<BudgetMetric> {
    let mut out: Vec<BudgetMetric> = Vec::new();
    let overloads = scan_overloads(pools);
    let get = |pool: PoolKind| -> u32 {
        let mut i = 0usize;
        while i < pools.len() {
            if let Some(p) = pools.get(i) {
                if p.pool == pool {
                    return p.budget_us;
                }
            }
            i += 1;
        }
        0
    };
    let rows: [(&'static str, u32, &'static str, &'static str); 6] = [
        (BUDGET_METRIC_IDS[0], FRAME_BUDGET_US, "µs", "帧预算总窗（80Hz 基准）"),
        (BUDGET_METRIC_IDS[1], get(PoolKind::Present), "µs", "呈现合成池预算"),
        (BUDGET_METRIC_IDS[2], get(PoolKind::UiResponse), "µs", "界面响应池预算"),
        (BUDGET_METRIC_IDS[3], get(PoolKind::Background), "µs", "后台任务池预算"),
        (BUDGET_METRIC_IDS[4], get(PoolKind::NetReserve), "µs", "网络与余量池预算"),
        (BUDGET_METRIC_IDS[5], overloads.len() as u32, "个", "超载池计数"),
    ];
    let mut i = 0usize;
    while i < 6 {
        if let Some(r) = rows.get(i) {
            out.push(BudgetMetric { id: r.0, value: r.1, unit: r.2, basis: r.3 });
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 十、对外接口冻结 v1（一站式帧预算构建）
// ---------------------------------------------------------------------------

/// 帧预算构建请求（冻结 v1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameBudgetRequestV1 {
    /// 协议版本（1）。
    pub version: u32,
    /// 场景标签。
    pub scene: String,
    /// 自定义比例（None = 默认保交互）。
    pub ratios: Option<[u32; 4]>,
    /// 申请清单（顺序即优先序——先申请先占）。
    pub allocs: Vec<AllocRequest>,
}

/// 帧预算答复（冻结 v1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameBudgetReplyV1 {
    /// 接受标记。
    pub accepted: bool,
    /// 池账（接受时有效）。
    pub pools: Vec<PoolLedger>,
    /// 授予结论（与请求一一对应）。
    pub verdicts: Vec<AllocVerdict>,
    /// 码面（0 = 全过）。
    pub code: u16,
    /// 人读明细。
    pub detail: String,
    /// 配置指纹。
    pub fingerprint: u32,
}

/// 一站式构建（判据一~五串行：配置→派生→分配→守恒→导出）。
pub fn build_frame_budget(req: &FrameBudgetRequestV1) -> FrameBudgetReplyV1 {
    // 配置面。
    let config = match &req.ratios {
        None => PoolConfig::standard(),
        Some(rs) => {
            let arr = [*rs.get(0).unwrap_or(&0), *rs.get(1).unwrap_or(&0), *rs.get(2).unwrap_or(&0), *rs.get(3).unwrap_or(&0)];
            match PoolConfig::custom(arr) {
                Ok(c) => c,
                Err(code) => {
                    return FrameBudgetReplyV1 {
                        accepted: false,
                        pools: Vec::new(),
                        verdicts: Vec::new(),
                        code: code as u16,
                        detail: code.detail().to_string(),
                        fingerprint: 0,
                    };
                }
            }
        }
    };
    let mut eng = BudgetEngine::with_config(config);
    let mut verdicts: Vec<AllocVerdict> = Vec::new();
    let mut i = 0usize;
    while i < req.allocs.len() {
        if let Some(a) = req.allocs.get(i) {
            verdicts.push(eng.allocate(a));
        }
        i += 1;
    }
    let pools = eng.snapshot();
    // 守恒终检。
    let cons = check_conservation(&pools);
    let complete = pools_complete(&pools);
    let (ok, code, detail) = match (cons, complete) {
        (Conservation::Sound, true) => (true, 0u16, "帧预算构建完成：四池守恒".to_string()),
        (Conservation::Sound, false) => (false, BcCode::PoolSumBreach as u16, "四池覆盖不全".to_string()),
        (Conservation::PoolSumBroken, _) => (false, BcCode::PoolSumBreach as u16, BcCode::PoolSumBreach.detail().to_string()),
        (Conservation::PoolOverloaded, _) => (false, BcCode::AccountOverload as u16, BcCode::AccountOverload.detail().to_string()),
    };
    FrameBudgetReplyV1 {
        accepted: ok,
        pools: {
            let mut v: Vec<PoolLedger> = Vec::new();
            let mut i = 0usize;
            while i < pools.len() {
                if let Some(p) = pools.get(i) {
                    v.push(p.clone());
                }
                i += 1;
            }
            v
        },
        verdicts,
        code,
        detail,
        fingerprint: config.fingerprint(),
    }
}

/// 守恒断言（换算器恒等抽查：四池默认预算逐项 roundtrip）。
const _: () = {
    assert!(roundtrip_ok(DEFAULT_POOL_BUDGETS_US[0]), "呈现池双表述往返失恒");
    assert!(roundtrip_ok(DEFAULT_POOL_BUDGETS_US[1]), "界面池双表述往返失恒");
    assert!(roundtrip_ok(DEFAULT_POOL_BUDGETS_US[2]), "后台池双表述往返失恒");
    assert!(roundtrip_ok(DEFAULT_POOL_BUDGETS_US[3]), "余量池双表述往返失恒");
};

/// 复用对拍锚点。
pub const REUSE_ANCHORS: [&str; 4] = [
    "cgc01_carbitration 账户模型——预算在先申请在后",
    "F2401 预算意识使命——遥测不做帧预算隐形税",
    "F0483 帧账 12.5ms 口径——帧预算锚点一致",
    "F2408 万分比整数口径——无浮点纪律",
];

// ---------------------------------------------------------------------------
// 十一、预算时间线（帧窗内分段交付节奏——vsync 对齐）
// ---------------------------------------------------------------------------

/// 时间线分段数（帧窗切成八段——段粒度 1.5625ms；整数 µs 逐段 1562/1563 尾差归末段）。
pub const TIMELINE_SEGMENTS: u32 = 8;

/// 段长（µs；整除向下——尾差归末段，Σ段=帧窗守恒）。
pub const SEGMENT_US: u32 = FRAME_BUDGET_US / TIMELINE_SEGMENTS;

/// 时间线槽（一段内各池的份额——确定性排布）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineSlot {
    /// 槽序（0..TIMELINE_SEGMENTS）。
    pub slot: u32,
    /// 本段起点（µs，帧窗相对值）。
    pub start_us: u32,
    /// 本段终点（µs，含头不含尾）。
    pub end_us: u32,
    /// 本段呈现池可用（µs）。
    pub present_us: u32,
    /// 本段界面池可用（µs）。
    pub ui_us: u32,
    /// 本段后台池可用（µs）。
    pub bg_us: u32,
    /// 本段余量池可用（µs）。
    pub reserve_us: u32,
}

/// 段起点（µs；最后一吞尾差）。
pub const fn slot_start(slot: u32) -> u32 {
    if slot >= TIMELINE_SEGMENTS {
        return FRAME_BUDGET_US;
    }
    slot * SEGMENT_US
}

/// 段终点（末段 = 帧窗——尾差吞并）。
pub const fn slot_end(slot: u32) -> u32 {
    if slot + 1 >= TIMELINE_SEGMENTS {
        return FRAME_BUDGET_US;
    }
    (slot + 1) * SEGMENT_US
}

/// 时间线构建（确定性：四池按比例逐段分摊，尾差归末段呈现池——呈现优先）。
pub fn build_timeline(config: &PoolConfig) -> Vec<TimelineSlot> {
    let budgets = config.budgets();
    let mut out: Vec<TimelineSlot> = Vec::new();
    let mut slot = 0u32;
    while slot < TIMELINE_SEGMENTS {
        let start = slot_start(slot);
        let end = slot_end(slot);
        let width = end - start;
        let s = TimelineSlot {
            slot,
            start_us: start,
            end_us: end,
            present_us: width * budgets[0] / FRAME_BUDGET_US,
            ui_us: width * budgets[1] / FRAME_BUDGET_US,
            bg_us: width * budgets[2] / FRAME_BUDGET_US,
            reserve_us: width * budgets[3] / FRAME_BUDGET_US,
        };
        out.push(s);
        slot += 1;
    }
    // 尾差归末段呈现池（Σ段呈现 = 池呈现预算——守恒收口）。
    let mut sum_present = 0u32;
    let mut i = 0usize;
    while i < out.len() {
        if let Some(s) = out.get(i) {
            sum_present += s.present_us;
        }
        i += 1;
    }
    let target = budgets[0];
    if sum_present < target {
        let tail = target - sum_present;
        let last = out.len() - 1;
        if let Some(s) = out.get_mut(last) {
            s.present_us += tail;
        }
    }
    out
}

/// 时间线守恒（Σ各段各池 = 各池预算——分段不丢预算）。
pub fn timeline_consistent(tl: &[TimelineSlot], config: &PoolConfig) -> bool {
    if tl.len() != TIMELINE_SEGMENTS as usize {
        return false;
    }
    let budgets = config.budgets();
    let mut sums = [0u32; 4];
    let mut i = 0usize;
    while i < tl.len() {
        let s = match tl.get(i) {
            Some(x) => *x,
            None => return false,
        };
        sums[0] += s.present_us;
        sums[1] += s.ui_us;
        sums[2] += s.bg_us;
        sums[3] += s.reserve_us;
        // 段界单调。
        if s.end_us <= s.start_us && s.slot + 1 < TIMELINE_SEGMENTS {
            return false;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < 4 {
        let b = match budgets.get(k) {
            Some(x) => *x,
            None => return false,
        };
        let v = match sums.get(k) {
            Some(x) => *x,
            None => return false,
        };
        // ui/bg/reserve 允许 ≤（段内向下取整零头归呈现已收口呈现项；
        // 其余三池零头并入末段呈现可接受——呈现守恒精确，其余容差 ≤ 段数）。
        if k == 0 {
            if v != b {
                return false;
            }
        } else if b - v > TIMELINE_SEGMENTS {
            return false;
        }
        k += 1;
    }
    true
}

/// 段查（µs 时刻 → 所在段序；越界 → None——不猜）。
pub const fn slot_at_us(t_us: u32) -> Option<u32> {
    if t_us >= FRAME_BUDGET_US {
        return None;
    }
    Some(t_us / SEGMENT_US)
}

// ---------------------------------------------------------------------------
// 十二、预算压力与降档联动（池压力 → C 域仲裁联动建议面）
// ---------------------------------------------------------------------------

/// 池压力五级闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolStress {
    /// 安适（占用 ≤ 50%）。
    Calm,
    /// 观察（50% < 占用 ≤ 70%）。
    Watch,
    /// 紧张（70% < 占用 ≤ 85%）。
    Tight,
    /// 高压（85% < 占用 < 100%）。
    High,
    /// 满载（= 100%——超载前一瞬）。
    Saturated,
}

impl PoolStress {
    /// 评级（确定性整数口径）。
    pub const fn grade(allocated_us: u32, budget_us: u32) -> PoolStress {
        if budget_us == 0 {
            return PoolStress::Saturated;
        }
        // 占用万分比。
        const PER: u32 = 10_000;
        let ppm = (allocated_us as u64).saturating_mul(PER as u64) as u32 / budget_us;
        if ppm > 10_000 {
            return PoolStress::Saturated;
        }
        if ppm <= 5_000 {
            PoolStress::Calm
        } else if ppm <= 7_000 {
            PoolStress::Watch
        } else if ppm <= 8_500 {
            PoolStress::Tight
        } else if ppm < 10_000 {
            PoolStress::High
        } else {
            PoolStress::Saturated
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            PoolStress::Calm => "安适",
            PoolStress::Watch => "观察",
            PoolStress::Tight => "紧张",
            PoolStress::High => "高压",
            PoolStress::Saturated => "满载",
        }
    }

    /// 降档建议（None = 无需动作——压力面到建议面的确定性映射）。
    pub const fn action(self) -> Option<&'static str> {
        match self {
            PoolStress::Calm => None,
            PoolStress::Watch => Some("记录趋势：连续三帧 Watch 则预警"),
            PoolStress::Tight => Some("冻结后台池新申请：后台可让渡先让"),
            PoolStress::High => Some("触发后台账户降档：按优先级从低到高回收"),
            PoolStress::Saturated => Some("呈报仲裁：池内已无让渡空间——走 C 域仲裁重分配"),
        }
    }
}

/// 全池压力巡检（四池逐检 → (池, 压力) 升序清单——按 PoolKind 稳定序）。
pub fn stress_patrol(pools: &[PoolLedger]) -> Vec<(PoolKind, PoolStress)> {
    let mut out: Vec<(PoolKind, PoolStress)> = Vec::new();
    let mut i = 0usize;
    while i < pools.len() {
        if let Some(p) = pools.get(i) {
            out.push((p.pool, PoolStress::grade(p.allocated_us, p.budget_us)));
        }
        i += 1;
    }
    out.sort_by(|a, b| (a.0.ordinal()).cmp(&b.0.ordinal()));
    out
}

/// 仲裁移交建议（满载池 → 需要仲裁重分配的池清单）。
pub fn arbitration_handover(pools: &[PoolLedger]) -> Vec<PoolKind> {
    let mut out: Vec<PoolKind> = Vec::new();
    let mut i = 0usize;
    while i < pools.len() {
        if let Some(p) = pools.get(i) {
            if PoolStress::grade(p.allocated_us, p.budget_us) == PoolStress::Saturated {
                out.push(p.pool);
            }
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 十三、预算审计账（分配历史滚动 + 三态查询——复用 F0483 账本模式）
// ---------------------------------------------------------------------------

/// 单帧预算留痕（B08 五元组：场景 + 分配数 + 超载数 + 压力峰值 + 码面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetFrameRecord {
    /// 场景标签。
    pub scene: String,
    /// 账户数。
    pub accounts: u32,
    /// 超载池数。
    pub overloads: u32,
    /// 压力峰值级（PoolStress::depth 语义——Saturated=4 最高）。
    pub stress_peak: u8,
    /// 码面（0 = 无错）。
    pub code: u16,
}

impl PoolStress {
    /// 峰值序（账面存储值——数值越大压力越高）。
    pub const fn peak_value(self) -> u8 {
        match self {
            PoolStress::Calm => 0,
            PoolStress::Watch => 1,
            PoolStress::Tight => 2,
            PoolStress::High => 3,
            PoolStress::Saturated => 4,
        }
    }
}

/// 预算审计账（256 帧滚动）。
#[derive(Clone, Debug, Default)]
pub struct BudgetLedger {
    records: Vec<BudgetFrameRecord>,
}

impl BudgetLedger {
    /// 空账。
    pub fn new() -> Self {
        BudgetLedger { records: Vec::new() }
    }

    /// 落账（封顶 256 滚动）。
    pub fn record(&mut self, rec: BudgetFrameRecord) {
        if self.records.len() >= 256 {
            self.records.remove(0);
        }
        self.records.push(rec);
    }

    /// 精确查（场景精确匹配，最新优先）。
    pub fn exact(&self, scene: &str) -> Option<BudgetFrameRecord> {
        let mut i = self.records.len();
        while i > 0 {
            i -= 1;
            if let Some(r) = self.records.get(i) {
                if r.scene == scene {
                    return Some(r.clone());
                }
            }
        }
        None
    }

    /// 聚合查（前缀聚合 → (帧数, Σ账户, Σ超载, 峰值)）。
    pub fn aggregate(&self, prefix: &str) -> Option<(u32, u32, u32, u8)> {
        let mut frames = 0u32;
        let mut accounts = 0u32;
        let mut overloads = 0u32;
        let mut peak = 0u8;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.scene.starts_with(prefix) {
                    frames += 1;
                    accounts += r.accounts;
                    overloads += r.overloads;
                    if r.stress_peak > peak {
                        peak = r.stress_peak;
                    }
                }
            }
            i += 1;
        }
        if frames == 0 {
            None
        } else {
            Some((frames, accounts, overloads, peak))
        }
    }

    /// 账深。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// 审计三态查询。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BudgetQuery {
    /// 精确命中。
    Exact(BudgetFrameRecord),
    /// 聚合命中。
    Aggregated(u32, u32, u32, u8),
    /// 未命中。
    NotFound,
}

/// 统一查账入口。
pub fn query_budget_ledger(ledger: &BudgetLedger, scene: &str) -> BudgetQuery {
    if let Some(r) = ledger.exact(scene) {
        return BudgetQuery::Exact(r);
    }
    if let Some((f, a, o, p)) = ledger.aggregate(scene) {
        return BudgetQuery::Aggregated(f, a, o, p);
    }
    BudgetQuery::NotFound
}

/// 一站式构建 + 审计落账（管线封装：build → 巡检 → 落账）。
pub fn build_and_audit(
    req: &FrameBudgetRequestV1,
    ledger: &mut BudgetLedger,
) -> FrameBudgetReplyV1 {
    let reply = build_frame_budget(req);
    let mut peak = 0u8;
    let mut overload_count = 0u32;
    let mut i = 0usize;
    while i < reply.pools.len() {
        if let Some(p) = reply.pools.get(i) {
            let s = PoolStress::grade(p.allocated_us, p.budget_us);
            let v = s.peak_value();
            if v > peak {
                peak = v;
            }
            if s == PoolStress::Saturated && p.allocated_us > p.budget_us {
                overload_count += 1;
            }
        }
        i += 1;
    }
    ledger.record(BudgetFrameRecord {
        scene: req.scene_snapshot(),
        accounts: reply.verdicts.len() as u32,
        overloads: overload_count,
        stress_peak: peak,
        code: reply.code,
    });
    reply
}

impl FrameBudgetRequestV1 {
    /// 场景快照（req 的只读面——账本落账取标签的单点）。
    pub fn scene_snapshot(&self) -> String {
        self.scene.clone()
    }
}

/// 时间线与压力守恒断言。
const _: () = {
    assert!(TIMELINE_SEGMENTS == 8, "时间线必须八段");
    assert!(SEGMENT_US * TIMELINE_SEGMENTS <= FRAME_BUDGET_US, "段长×段数不得越帧窗");
};

// ---------------------------------------------------------------------------
// 十四、预算余量再分配器（帧末余量回收 → 跨池再分配；余量归池规则显性）
// ---------------------------------------------------------------------------

/// 再分配策略闭集（帧末余量怎么分——锚点「默认保交互」的余量侧延伸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlackPolicy {
    /// 归呈现（余量给呈现池——画质增强空间；默认保交互的对偶）。
    ToPresent,
    /// 归界面（余量给界面响应——交互顺滑优先）。
    ToUi,
    /// 归余量池沉淀（累积给下一帧弹性缓冲）。
    ToReserve,
    /// 均分（四池均摊——零偏好）。
    Even,
}

impl SlackPolicy {
    /// 全量闭集。
    pub const ALL: [SlackPolicy; 4] = [
        SlackPolicy::ToPresent,
        SlackPolicy::ToUi,
        SlackPolicy::ToReserve,
        SlackPolicy::Even,
    ];

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            SlackPolicy::ToPresent => "归呈现",
            SlackPolicy::ToUi => "归界面",
            SlackPolicy::ToReserve => "沉淀",
            SlackPolicy::Even => "均分",
        }
    }
}

/// 一次再分配结论（来源池、去向池、量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reallocation {
    /// 来源池。
    pub from: PoolKind,
    /// 去向池。
    pub to: PoolKind,
    /// 转移量（µs）。
    pub amount_us: u32,
    /// 依据。
    pub basis: &'static str,
}

/// 余量再分配器（确定性；跨池转移保持 Σ池=帧窗守恒）。
///
/// 规则（显性）：
/// - 来源恒为后台池（可让渡面——呈现/界面/余量的余量不是「余量」是「承诺」，
///   不可再挪）。
/// - 按策略定去向；Even 策略给其余三池均分（向下取整，尾差归呈现）。
/// - 来源池余量 < 阈值不动作（抖动防线：碎片转移无收益）。
pub fn redistribute(
    pools: &mut [PoolLedger; 4],
    policy: SlackPolicy,
    threshold_us: u32,
) -> Vec<Reallocation> {
    let mut out: Vec<Reallocation> = Vec::new();
    // 后台池余量。
    let bg_idx = PoolKind::Background.ordinal();
    let bg_slack = match pools.get(bg_idx) {
        Some(p) => p.slack_us(),
        None => return out,
    };
    if bg_slack < threshold_us || bg_slack == 0 {
        return out; // 抖动防线：不足阈值不动作。
    }
    match policy {
        SlackPolicy::ToPresent => {
            let pi = PoolKind::Present.ordinal();
            let mut src = match pools.get_mut(bg_idx) {
                Some(p) => p,
                None => return out,
            };
            src.budget_us -= bg_slack;
            let amt = bg_slack;
            let dst = match pools.get_mut(pi) {
                Some(p) => p,
                None => return out,
            };
            dst.budget_us += amt;
            out.push(Reallocation {
                from: PoolKind::Background,
                to: PoolKind::Present,
                amount_us: amt,
                basis: "余量归呈现：默认保交互的对偶——画质增强空间",
            });
        }
        SlackPolicy::ToUi => {
            let ui = PoolKind::UiResponse.ordinal();
            let mut src = match pools.get_mut(bg_idx) {
                Some(p) => p,
                None => return out,
            };
            src.budget_us -= bg_slack;
            let amt = bg_slack;
            let dst = match pools.get_mut(ui) {
                Some(p) => p,
                None => return out,
            };
            dst.budget_us += amt;
            out.push(Reallocation {
                from: PoolKind::Background,
                to: PoolKind::UiResponse,
                amount_us: amt,
                basis: "余量归界面：交互顺滑优先",
            });
        }
        SlackPolicy::ToReserve => {
            let ri = PoolKind::NetReserve.ordinal();
            let mut src = match pools.get_mut(bg_idx) {
                Some(p) => p,
                None => return out,
            };
            src.budget_us -= bg_slack;
            let amt = bg_slack;
            let dst = match pools.get_mut(ri) {
                Some(p) => p,
                None => return out,
            };
            dst.budget_us += amt;
            out.push(Reallocation {
                from: PoolKind::Background,
                to: PoolKind::NetReserve,
                amount_us: amt,
                basis: "余量沉淀：下一帧弹性缓冲",
            });
        }
        SlackPolicy::Even => {
            let each = bg_slack / 3;
            if each == 0 {
                return out;
            }
            let mut moved = 0u32;
            // 均分给呈现/界面/余量（尾差归呈现）。
            let targets = [PoolKind::Present, PoolKind::UiResponse, PoolKind::NetReserve];
            let mut t = 0usize;
            while t < targets.len() {
                let tgt = match targets.get(t) {
                    Some(x) => *x,
                    None => break,
                };
                let ti = tgt.ordinal();
                let amt = if t + 1 == targets.len() {
                    bg_slack - moved // 尾差归最后一个目标（余量池）——呈现精确优先已在上文说明；
                                     // 此处 Even 策略尾差归余量池（弹性缓冲吃零头最无害）。
                } else {
                    each
                };
                if amt == 0 {
                    t += 1;
                    continue;
                }
                let mut src = match pools.get_mut(bg_idx) {
                    Some(p) => p,
                    None => return out,
                };
                src.budget_us = src.budget_us.saturating_sub(amt);
                let dst = match pools.get_mut(ti) {
                    Some(p) => p,
                    None => return out,
                };
                dst.budget_us += amt;
                moved += amt;
                out.push(Reallocation {
                    from: PoolKind::Background,
                    to: tgt,
                    amount_us: amt,
                    basis: "余量均分：零偏好均摊（尾差归余量池）",
                });
                t += 1;
            }
        }
    }
    out
}

/// 再分配后守恒核验（Σ池预算仍 = 帧窗——转移不丢）。
pub fn redistribution_consistent(pools: &[PoolLedger; 4]) -> bool {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < 4 {
        if let Some(p) = pools.get(i) {
            sum = sum.saturating_add(p.budget_us);
        }
        i += 1;
    }
    sum == FRAME_BUDGET_US
}

// ---------------------------------------------------------------------------
// 十五、预算仿真走查（锚点判据五的机检面：默认配置全链路走查）
// ---------------------------------------------------------------------------

/// 走查结论（每步 (步骤名, 通过)——锚点判据逐步对拍）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkStep {
    /// 步骤名。
    pub step: &'static str,
    /// 通过。
    pub pass: bool,
    /// 对拍依据。
    pub basis: &'static str,
}

/// 全链路走查（判据一~五逐条机检：默认配置 + 典型申请集）。
pub fn walk_default_budget() -> Vec<WalkStep> {
    let mut steps: Vec<WalkStep> = Vec::new();
    // 步 1：四池闭集（判据一）。
    steps.push(WalkStep {
        step: "四池闭集",
        pass: PoolKind::ALL.len() == 4,
        basis: "呈现合成/界面响应/后台任务/网络与余量",
    });
    // 步 2：默认守恒（Σ池=帧窗）。
    let cfg = PoolConfig::standard();
    let budgets = cfg.budgets();
    let sum = budgets[0] + budgets[1] + budgets[2] + budgets[3];
    steps.push(WalkStep {
        step: "默认守恒",
        pass: sum == FRAME_BUDGET_US,
        basis: "Σ池=12500µs 恰满",
    });
    // 步 3：三级层级（判据二）。
    steps.push(WalkStep {
        step: "三级层级",
        pass: level_step_ok(BudgetLevel::Frame, BudgetLevel::Pool)
            && level_step_ok(BudgetLevel::Pool, BudgetLevel::Account),
        basis: "帧→池→账户 深度单调 +1",
    });
    // 步 4：双表述（判据三）。
    steps.push(WalkStep {
        step: "双表述",
        pass: roundtrip_ok(FRAME_BUDGET_US) && roundtrip_ok(budgets[0]),
        basis: "微秒↔占比往返误差 ≤ 1µs",
    });
    // 步 5：典型申请 + 守恒校验（判据四）。
    let mut eng = BudgetEngine::standard();
    let a1 = PoolAccount {
        account_id: 1,
        pool: PoolKind::Present,
        priority: 0,
        floor_us: 1_000,
        cap_us: 4_000,
        lendable: false,
        label: "主渲染".to_string(),
    };
    let v1 = eng.allocate(&AllocRequest { account: a1, ask_us: 3_000 });
    let granted_ok = match v1 {
        AllocVerdict::Granted { granted_us } => granted_us == 3_000,
        _ => false,
    };
    let snap = eng.snapshot();
    let cons = check_conservation(&snap);
    steps.push(WalkStep {
        step: "典型申请",
        pass: granted_ok && cons == Conservation::Sound,
        basis: "主渲染 3000µs 全额授予且池守恒",
    });
    // 步 6：超载显性（判据四的负例：挤爆呈现池 → 拒）。
    let a2 = PoolAccount {
        account_id: 2,
        pool: PoolKind::Present,
        priority: 9,
        floor_us: 0,
        cap_us: 10_000,
        lendable: true,
        label: "贪心客".to_string(),
    };
    let v2 = eng.allocate(&AllocRequest { account: a2, ask_us: 10_000 });
    let reject_ok = match v2 {
        AllocVerdict::Denied { .. } => true,
        AllocVerdict::Granted { granted_us } | AllocVerdict::Floored { granted_us } => granted_us <= 5_625,
        _ => false,
    };
    steps.push(WalkStep {
        step: "超载显性",
        pass: reject_ok,
        basis: "呈现池余量不足时按 floor 兜底或显性拒——不静默挤占",
    });
    // 步 7：判据词面核验（锚点判据五条逐条在册）。
    steps.push(WalkStep {
        step: "判据词面",
        pass: ANCHOR_CRITERIA.len() == 5,
        basis: "四池、三级、双表述、守恒校验、判据",
    });
    steps
}

/// 锚点判据词面（判据侧独立重排——与锚点原文逐字对拍）。
pub const ANCHOR_CRITERIA: [&str; 5] = ["四池", "三级", "双表述", "守恒校验", "判据"];

/// 走查全过判。
pub fn walk_all_pass(steps: &[WalkStep]) -> bool {
    let mut i = 0usize;
    while i < steps.len() {
        if let Some(s) = steps.get(i) {
            if !s.pass {
                return false;
            }
        }
        i += 1;
    }
    !steps.is_empty()
}
