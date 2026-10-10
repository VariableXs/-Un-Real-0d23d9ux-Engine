//! CGPU-F1123 · 预算池分级（CGPU-H 域 · 显存与预算域 · H03 组 · 目标 340 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1123`
//!
//! **判据（锚点原文）**：三级、账户树、守恒、水位、判据。
//!
//! **职责定位（锚点原文）**：池分级：三级池（进程池（各引擎会话）/系统池
//! （系统 UI/合成器）/保留池（应急+驱动保留）——三级语义与配额）；池配额
//! 分配（进程内再分账户——复用 C 域账户树理念）；池校验（Σ配额≤总显存
//! ——守恒复用）；池水位（三级池各自水位监控——告警联动）。
//!
//! ## 一、三级池闭集与语义（判据一）
//!
//! 三级（[`PoolTier`]）：进程池（各引擎会话的显存主战场）/系统池
//! （系统 UI 与合成器——交互命脉，不让度）/保留池（应急+驱动保留，
//! 平时不动）。三级语义绑定缺省配额比例（[`DEFAULT_QUOTAS_MB`]）与
//! 不可让度规则——语义与配额一体，不是三个名字。
//!
//! ## 二、账户树复用（判据二：进程内再分账户）
//!
//! 进程池内再分账户（[`PoolAccount`]：账户 id/进程标签/配额/已用），
//! 账户模型复用 C 域账户树理念（F0323）——复用是口径复用（配额/
//! 已用/守恒三概念同名同义），不重抄实现。账户申请走显性校验：
//! 超池配额拒绝（[`XcCode::QuotaExceeded`]）并计数。
//!
//! ## 三、守恒校验（判据三：Σ配额 ≤ 总显存）
//!
//! [`PoolBook::conserve`]：三级池配额之和 ≤ 总显存（[`total_vram_mb`]
//! 注入——零墙钟同理零硬探测，总显存由上游探测面注入）；配额溢出
//! 显性拒（不静默钳制——静默钳制会把配置错误藏进运行期）。
//!
//! ## 四、水位监控与告警联动（判据四）
//!
//! 四级水位（[`WaterLevel`]：绿/黄/橙/红）按占用比对分级
//! （[`watermark_of`]，阈值常量写死并注释来源）；告警联动接口
//! （[`PoolBook::alerts`]）产出逐池告警行——红位触发 H04 回收联动
//! （联动声明，实现位在 F1124）。
//!
//! ## 五、记账一致性
//!
//! 池账（[`PoolBook`]）双守恒：池配额守恒（判据三）+ 账户对池守恒
//! （Σ账户已用 ≤ 池已用——账户账不超池账）；双守恒全过才算账平。
//!
//! **对接**：F1122（显存账本——总量与占用来源）；F1124（水位联动
//! 回收/换页/降质——告警消费方）；F0323（C 域账户树——复用对象）。
//! 零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（配额与水位阈值唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const POOL_TIER_VERSION: &str = "H03-pooltier-v1";

/// 缺省三级配额（MB）：进程/系统/保留。来源：本域规划缺省值
/// （进程 70%/系统 20%/保留 10%）；当前策略值，可调。
pub const DEFAULT_QUOTAS_MB: [u64; 3] = [700, 200, 100];

/// 水位阈值（占池配额百分比）：黄 ≥70%，橙 ≥85%，红 ≥95%。
/// 来源：复用 E04 F0692 水位理念的显存版分级锚点。
pub const WATERMARK_YELLOW_PCT: u64 = 70;
pub const WATERMARK_ORANGE_PCT: u64 = 85;
pub const WATERMARK_RED_PCT: u64 = 95;

// ---------------------------------------------------------------------------
// 二、三级池闭集（判据一）
// ---------------------------------------------------------------------------

/// 三级池闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolTier {
    /// 进程池：各引擎会话的显存主战场。
    Process,
    /// 系统池：系统 UI/合成器——交互命脉，不可让度。
    System,
    /// 保留池：应急+驱动保留，平时不动。
    Reserve,
}

impl PoolTier {
    /// 池序（0-2；配额表/水位表下标）。
    pub const fn ordinal(self) -> usize {
        match self {
            PoolTier::Process => 0,
            PoolTier::System => 1,
            PoolTier::Reserve => 2,
        }
    }

    /// 池语义（三级语义与配额一体）。
    pub const fn semantics(self) -> &'static str {
        match self {
            PoolTier::Process => "进程池：各引擎会话显存；可按账户树再分；可让度",
            PoolTier::System => "系统池：系统UI/合成器；交互命脉；不可让度",
            PoolTier::Reserve => "保留池：应急+驱动保留；平时不动；动用即告警",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、诊断码（0x65 段独占：H03 池分级面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x65 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 池配额之和超总显存。
    QuotaOverVram,
    /// 账户申请超池配额。
    QuotaExceeded,
    /// 账户账超池账（双守恒第二式失败）。
    AccountOverPool,
    /// 未知池序（表外池）。
    UnknownTier,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::QuotaOverVram => "池配额之和超总显存：守恒式 Σ配额≤总显存不成立，建议下调池配额而非静默钳制",
            XcCode::QuotaExceeded => "账户申请超池配额：进程内账户不得突破所属池配额，建议收缩账户配额或申请池间再平衡",
            XcCode::AccountOverPool => "账户账超池账：Σ账户已用>池已用属记账断裂，建议对账修复后再分配",
            XcCode::UnknownTier => "未知池序：三级闭集之外无池，建议按 PoolTier 闭集取用",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、账户树复用（判据二）
// ---------------------------------------------------------------------------

/// 池内账户（复用 C 域账户树理念：配额/已用/守恒同名同义）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolAccount {
    /// 账户 id（进程池内唯一）。
    pub account_id: u32,
    /// 进程标签（归属会话）。
    pub owner: String,
    /// 账户配额（MB；≤所属池配额）。
    pub quota_mb: u64,
    /// 已用（MB；≤账户配额）。
    pub used_mb: u64,
}

/// 账户申请（校验→入账，超配额显性拒）。
pub fn account_request(
    acc: &mut PoolAccount,
    pool_quota_mb: u64,
    pool_used_mb: u64,
    want_mb: u64,
    rejected: &mut u64,
) -> Result<u64, XcCode> {
    let new_used = acc.used_mb.saturating_add(want_mb);
    if new_used > acc.quota_mb || acc.quota_mb > pool_quota_mb {
        *rejected = rejected.saturating_add(1);
        return Err(XcCode::QuotaExceeded);
    }
    let new_pool_used = pool_used_mb.saturating_add(want_mb);
    if new_pool_used > pool_quota_mb {
        *rejected = rejected.saturating_add(1);
        return Err(XcCode::QuotaExceeded);
    }
    acc.used_mb = new_used;
    Ok(acc.used_mb)
}

// ---------------------------------------------------------------------------
// 五、水位（判据四）
// ---------------------------------------------------------------------------

/// 四级水位（复用 E04 F0692 理念的显存版）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterLevel {
    /// 绿：充裕。
    Green,
    /// 黄：关注。
    Yellow,
    /// 橙：预警。
    Orange,
    /// 红：告警（联动回收/降质）。
    Red,
}

impl WaterLevel {
    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            WaterLevel::Green => "绿",
            WaterLevel::Yellow => "黄",
            WaterLevel::Orange => "橙",
            WaterLevel::Red => "红",
        }
    }
}

/// 水位判定（占用比对分级；阈值来源见常量注释）。
pub const fn watermark_of(used_mb: u64, quota_mb: u64) -> WaterLevel {
    if quota_mb == 0 {
        // 零配额池没有可占用面——按绿处理（不存在占满问题）。
        return WaterLevel::Green;
    }
    // 占用百分比：used*100/quota（u64 中间量，配额 MB 级不会溢出）。
    let pct = used_mb.saturating_mul(100) / quota_mb;
    if pct >= WATERMARK_RED_PCT {
        WaterLevel::Red
    } else if pct >= WATERMARK_ORANGE_PCT {
        WaterLevel::Orange
    } else if pct >= WATERMARK_YELLOW_PCT {
        WaterLevel::Yellow
    } else {
        WaterLevel::Green
    }
}

// ---------------------------------------------------------------------------
// 六、池账（判据三守恒 + 双守恒）
// ---------------------------------------------------------------------------

/// 单池账（配额/已用/账户树）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolBook {
    /// 三池配额（MB；下标 = PoolTier ordinal）。
    pub quotas_mb: [u64; 3],
    /// 三池已用（MB）。
    pub used_mb: [u64; 3],
    /// 进程池账户树（判据二：进程内再分账户）。
    pub accounts: Vec<PoolAccount>,
    /// 拒绝计数（超配额显性留痕）。
    pub rejected: u64,
}

impl PoolBook {
    /// 新池账（带缺省配额）。
    pub fn with_default_quotas() -> PoolBook {
        PoolBook {
            quotas_mb: DEFAULT_QUOTAS_MB,
            used_mb: [0; 3],
            accounts: Vec::new(),
            rejected: 0,
        }
    }

    /// 守恒校验（判据三：Σ配额 ≤ 总显存；总显存上游注入）。
    pub fn conserve(&self, total_vram_mb: u64) -> Result<(), XcCode> {
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < 3 {
            sum = sum.saturating_add(*self.quotas_mb.get(i).unwrap_or(&0));
            i += 1;
        }
        if sum > total_vram_mb {
            return Err(XcCode::QuotaOverVram);
        }
        Ok(())
    }

    /// 双守恒第二式：Σ账户已用 ≤ 进程池已用（账平才算账）。
    pub fn accounts_fit_pool(&self) -> Result<(), XcCode> {
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                sum = sum.saturating_add(a.used_mb);
            }
            i += 1;
        }
        let pool_used = *self.used_mb.get(PoolTier::Process.ordinal()).unwrap_or(&0);
        if sum > pool_used {
            return Err(XcCode::AccountOverPool);
        }
        Ok(())
    }

    /// 开账户（进程池内；超池配额即拒）。
    pub fn open_account(&mut self, account_id: u32, owner: &str, quota_mb: u64) -> Result<(), XcCode> {
        let pool_quota = *self.quotas_mb.get(PoolTier::Process.ordinal()).unwrap_or(&0);
        if quota_mb > pool_quota {
            self.rejected = self.rejected.saturating_add(1);
            return Err(XcCode::QuotaExceeded);
        }
        self.accounts.push(PoolAccount {
            account_id,
            owner: owner.to_string(),
            quota_mb,
            used_mb: 0,
        });
        Ok(())
    }

    /// 账户申请显存（走 [`account_request`] 校验面）。
    pub fn account_alloc(&mut self, account_id: u32, want_mb: u64) -> Result<u64, XcCode> {
        let pool_quota = *self.quotas_mb.get(PoolTier::Process.ordinal()).unwrap_or(&0);
        let pool_used = *self.used_mb.get(PoolTier::Process.ordinal()).unwrap_or(&0);
        // 取账户（无下标 panic 面：逐个比对）。
        let mut target: Option<usize> = None;
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(a) = self.accounts.get(i) {
                if a.account_id == account_id {
                    target = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let idx = match target {
            Some(i) => i,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(XcCode::QuotaExceeded);
            }
        };
        // 拆借用：账户可变借用与池已用读取分离。
        let (quota_before, used_before) = {
            let a = match self.accounts.get(idx) {
                Some(a) => (a.quota_mb, a.used_mb),
                None => return Err(XcCode::QuotaExceeded),
            };
            a
        };
        let mut acc = PoolAccount {
            account_id,
            owner: String::new(),
            quota_mb: quota_before,
            used_mb: used_before,
        };
        match account_request(&mut acc, pool_quota, pool_used, want_mb, &mut self.rejected) {
            Ok(new_used) => {
                if let Some(a) = self.accounts.get_mut(idx) {
                    a.used_mb = new_used;
                }
                if let Some(u) = self.used_mb.get_mut(PoolTier::Process.ordinal()) {
                    *u = u.saturating_add(want_mb);
                }
                Ok(new_used)
            }
            Err(c) => Err(c),
        }
    }

    /// 三池水位快照（判据四：各自水位监控）。
    pub fn watermarks(&self) -> [WaterLevel; 3] {
        [
            watermark_of(
                *self.used_mb.get(0).unwrap_or(&0),
                *self.quotas_mb.get(0).unwrap_or(&0),
            ),
            watermark_of(
                *self.used_mb.get(1).unwrap_or(&0),
                *self.quotas_mb.get(1).unwrap_or(&0),
            ),
            watermark_of(
                *self.used_mb.get(2).unwrap_or(&0),
                *self.quotas_mb.get(2).unwrap_or(&0),
            ),
        ]
    }

    /// 告警联动行（判据四：非绿水位产出告警；红位触发 H04 联动——声明）。
    pub fn alerts(&self) -> Vec<String> {
        let mut out = Vec::new();
        let levels = self.watermarks();
        let mut i = 0usize;
        while i < levels.len() {
            let tier = match i {
                0 => PoolTier::Process,
                1 => PoolTier::System,
                _ => PoolTier::Reserve,
            };
            let lv = match levels.get(i) {
                Some(l) => *l,
                None => break,
            };
            match lv {
                WaterLevel::Green => {}
                WaterLevel::Yellow => out.push(alloc::format!(
                    "[黄] {} 占用关注：{}MB/{}MB",
                    tier.semantics(),
                    self.used_mb.get(i).copied().unwrap_or(0),
                    self.quotas_mb.get(i).copied().unwrap_or(0)
                )),
                WaterLevel::Orange => out.push(alloc::format!(
                    "[橙] {} 预警：准备回收/换页联动（F1124）",
                    tier.semantics()
                )),
                WaterLevel::Red => out.push(alloc::format!(
                    "[红] {} 告警：触发 H04 回收/降质联动；保留池动用即告警",
                    tier.semantics()
                )),
            }
            i += 1;
        }
        out
    }
}
