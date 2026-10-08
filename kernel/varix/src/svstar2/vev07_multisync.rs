//! VE-F4407 · 多屏色彩同步（VE-W 域 · 显示与色彩批 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4407`
//!
//! **判据（锚点原文）**：派生联动、漂移监测、独立豁免、告警三通道、判据。
//!
//! **职责定位（锚点原文）**：多屏色彩同步机制（跨屏一致观感三件：
//! **配置联动**（主屏校准派生副屏）/**漂移监测**（周期采样对拍）/
//! **单屏独立豁免**（创作屏可豁免同步并标注））。降级矩阵：派生失配→
//! 逐屏重算；漂移超阈→告警+校准建议；豁免滥用→频次提示。性能：
//! 联动 O(屏数)；采样 O(1)；告警 O(1)。
//!
//! # 一、为什么派生是「主屏→副屏」单向而不是各屏协商
//!
//! 多屏观感不一致时总要有一个基准：协商没有天然裁判，会互相拉扯
//! （A 跟 B、B 跟 A 的回环收敛不了）。主屏校准（用户显式调过或
//! F4405 档案里最新生效的）为唯一真源，副屏**派生**其参数——方向
//! 单一，不一致时责任链清晰。派生失配（副屏现存值与派生值不同）
//! 不算事故：逐屏**重算**覆盖并留痕（锚点降级矩阵第一条）——重算
//! 是同步机制的正常工作模式，不是异常路径。
//!
//! # 二、漂移监测为什么只采参数不采内容
//!
//! 漂移（副屏随时间/温度偏离派生基准）的证据在校准参数的偏差里，
//! 不在画面内容里——采样只读 gamma/profile 参数（O(1) 单点对拍），
//! **不含任何像素或内容**（锚点隐私口径）。超阈（[`DRIFT_TOL_PER_MILLE`]）
//! → 告警 + 校准建议（不自动重校——校准是用户决策，F4425 深化）。
//! 阈内漂移记账不告警：漂移是缓慢物理过程，逐 tick 告警是狼来了。
//!
//! # 三、豁免为什么带频次提示
//!
//! 创作屏（调色/印刷）要求与主屏**不同**的校准——豁免是合法需求，
//! 但豁免-恢复反复横跳会让「哪些屏在同步」永远说不清（豁免滥用）。
//! 豁免登记（[`Exemption`]）带切换计数：一个同步周期内切换超
//! [`FLIP_FLOP_LIMIT`] 次即发频次提示（不阻止——用户意志优先，
//! 但要显性提示）。豁免屏在联动时**跳过**派生并标注——豁免不是
//! 隐藏，是带标注的例外。
//!
//! # 四、告警为什么三通道齐备
//!
//! 单通道告警必丢：读屏文本给不到不看屏幕的用户，台账立案给不到
//! 当场想处理的用户，建议文本没着头绪就是废话。[`DriftAlert`] 三字段
//! 齐发：读屏行（[`DriftAlert::screen_line`]）+ 台账立案（alerts 追加）+
//! 校准建议（advice 人话）——三通道是**同一事实**的三种呈现，不是
//! 三次独立判断（域本色，锚点无障碍口径）。
//!
//! # 五、对接（锚点原文）
//!
//! 上游 F4405 配置档案（联动表从档案派生）；F4425 校准深化、
//! F4426 HDR 多屏协调（前向声明）。性能：联动 O(屏数)（逐屏派生）、
//! 采样 O(1)（单点对拍）、告警 O(1)（单事件立案）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、错误码（字符串码家族）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SYNC_ENGINE_VERSION: &str = "V07-sync-v1";

/// 派生失配（已逐屏重算并留痕）。
pub const E_SYNC_DERIVE: &str = "E_SYNC_DERIVE";
/// 漂移超阈（告警 + 校准建议）。
pub const E_SYNC_DRIFT: &str = "E_SYNC_DRIFT";
/// 豁免频次提示（切换过频——显性不阻止）。
pub const E_SYNC_EXEMPT: &str = "E_SYNC_EXEMPT";
/// 输入非法（空主屏/参数越域）。
pub const E_SYNC_INPUT: &str = "E_SYNC_INPUT";

// ---------------------------------------------------------------------------
// 二、段一：配置联动（O(屏数)）
// ---------------------------------------------------------------------------

/// 联动配置表（数据结构之一：主屏基准 + 副屏清单 + 豁免登记表）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncConfig {
    /// 主屏身份键（派生源，唯一基准）。
    pub primary: String,
    /// 副屏身份键清单（派生目标）。
    pub secondaries: Vec<String>,
    /// 主屏校准 gamma（千分位，F4405 校准节口径 800..=1200）。
    pub gamma_per_mille: i64,
    /// 主屏校准 profile 短码（F4403 引用域）。
    pub color_profile: String,
}

impl SyncConfig {
    /// 参数合法性（gamma 域与 F4405 白名单一致；空主屏拒）。
    pub fn legal(&self) -> bool {
        !self.primary.is_empty()
            && self.gamma_per_mille >= 800
            && self.gamma_per_mille <= 1200
            && (self.color_profile == "srgb"
                || self.color_profile == "p3"
                || self.color_profile == "adobe"
                || self.color_profile == "custom")
    }
}

/// 单屏派生结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeriveOutcome {
    /// 与派生值一致（无需重算）。
    InSync,
    /// 派生失配 → 已逐屏重算覆盖（留痕）。
    Recalced { old_gamma: i64, new_gamma: i64 },
    /// 豁免屏：跳过派生并标注（创作屏独立校准合法）。
    Exempted,
}

/// 配置联动（O(屏数)：逐副屏派生主屏校准参数）。
///
/// 豁免屏跳过；失配屏重算覆盖留痕（E_SYNC_DERIVE 语义在案）；
/// 配置非法 → Err(E_SYNC_INPUT)——空基准不派生。
pub fn derive_all(
    cfg: &SyncConfig,
    exemptions: &[String],
    current_gamma: &dyn Fn(&str) -> Option<i64>,
) -> Result<Vec<(String, DeriveOutcome)>, &'static str> {
    if !cfg.legal() {
        return Err(E_SYNC_INPUT);
    }
    let mut out = Vec::new();
    for sec in &cfg.secondaries {
        if exemptions.iter().any(|e| e == sec) {
            out.push((sec.clone(), DeriveOutcome::Exempted));
            continue;
        }
        match current_gamma(sec) {
            Some(g) if g == cfg.gamma_per_mille => {
                out.push((sec.clone(), DeriveOutcome::InSync));
            }
            Some(g) => {
                out.push((sec.clone(), DeriveOutcome::Recalced { old_gamma: g, new_gamma: cfg.gamma_per_mille }));
            }
            None => {
                out.push((sec.clone(), DeriveOutcome::Recalced { old_gamma: 0, new_gamma: cfg.gamma_per_mille }));
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 三、段二：漂移监测（数据结构之二：监测流；采样 O(1)）
// ---------------------------------------------------------------------------

/// 漂移容差（千分位/gamma）：超过即告警+建议。
pub const DRIFT_TOL_PER_MILLE: i64 = 15;

/// 单次采样（**只含校准参数，不含内容**——隐私口径，结构可断言）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 屏身份键。
    pub identity: String,
    /// 采样时 gamma（千分位）。
    pub gamma_per_mille: i64,
    /// 采样 tick（逻辑时钟，零墙钟）。
    pub tick: u32,
}

/// 漂移监测流（周期采样对拍主屏基准）。
#[derive(Clone, Debug, Default)]
pub struct DriftMonitor {
    /// 采样流（只追加——对拍历史可回放）。
    pub stream: Vec<Sample>,
    /// 超阈告警计数。
    pub alerts: u32,
}

impl DriftMonitor {
    /// 单点采样对拍（O(1)：push + 单次比较；`baseline` 为主屏 gamma）。
    ///
    /// 返回 Some(漂移量) 当超阈（告警由调用方接 [`DriftAlert`] 三通道）；
    /// 阈内返回 None（记账不告警——漂移是缓慢物理过程）。
    pub fn sample(&mut self, identity: &str, gamma_per_mille: i64, baseline: i64, tick: u32) -> Option<i64> {
        self.stream.push(Sample { identity: identity.to_string(), gamma_per_mille, tick });
        let drift = (gamma_per_mille - baseline).abs();
        if drift > DRIFT_TOL_PER_MILLE {
            self.alerts = self.alerts.saturating_add(1);
            Some(drift)
        } else {
            None
        }
    }
}

/// 漂移告警（**三通道齐备**：读屏行 + 台账立案 + 校准建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriftAlert {
    /// 通道一：读屏单行（人话、可播报）。
    pub screen_line: String,
    /// 通道二：台账立案码（alerts 追加的证据键）。
    pub case_code: &'static str,
    /// 通道三：校准建议（人话下一步）。
    pub advice: String,
}

/// 三通道告警构造（O(1)：同一事实三种呈现，非三次独立判断）。
pub fn drift_alert(identity: &str, drift: i64) -> DriftAlert {
    DriftAlert {
        screen_line: format!(
            "漂移告警：显示器 {} 偏离主屏校准 {} 千分位（{}）",
            identity, drift, E_SYNC_DRIFT
        ),
        case_code: E_SYNC_DRIFT,
        advice: format!(
            "建议对 {} 重新校准，或检查显示器预热/线缆（漂移超容差 {}）",
            identity, DRIFT_TOL_PER_MILLE
        ),
    }
}

// ---------------------------------------------------------------------------
// 四、段三：单屏独立豁免（数据结构之三：豁免登记）
// ---------------------------------------------------------------------------

/// 豁免频次提示阈值（同一屏切换豁免超此数即提示）。
pub const FLIP_FLOP_LIMIT: u32 = 3;

/// 豁免登记（创作屏独立校准的显性例外）。
///
/// 撤销**不删记录**：`active` 翻转、`toggle_count` 跨撤销保留——
/// 否则反复横跳的计数每次归一，频次提示（E_SYNC_EXEMPT）永不触发
/// （恒假路径）。登记簿是历史，不是当前态的缓存。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exemption {
    /// 屏身份键。
    pub identity: String,
    /// 豁免原因（人话，如「调色创作屏」）。
    pub reason: String,
    /// 当前是否处于豁免态（撤销只翻此位，记录与计数保留）。
    pub active: bool,
    /// 豁免-恢复切换计数（滥用频次提示依据，跨撤销累计）。
    pub toggle_count: u32,
}

/// 豁免登记簿（O(1) 登记/撤销；频次提示显性不阻止）。
#[derive(Clone, Debug, Default)]
pub struct ExemptionBook {
    pub entries: Vec<Exemption>,
    /// 频次提示计数（诊断面板）。
    pub hints: u32,
}

impl ExemptionBook {
    /// 登记/恢复切换（O(1)：已登记则翻转 active、未登记则新建；返回频次提示）。
    ///
    /// 切换计数超 [`FLIP_FLOP_LIMIT`] → E_SYNC_EXEMPT 频次提示
    /// （显性不阻止——用户意志优先）。计数跨撤销累计（见 [`Exemption`]）。
    pub fn toggle(&mut self, identity: &str, reason: &str) -> Option<&'static str> {
        if let Some(e) = self.entries.iter_mut().find(|e| e.identity == identity) {
            e.active = !e.active;
            e.toggle_count = e.toggle_count.saturating_add(1);
            if e.toggle_count > FLIP_FLOP_LIMIT {
                self.hints = self.hints.saturating_add(1);
                Some(E_SYNC_EXEMPT)
            } else {
                None
            }
        } else {
            self.entries.push(Exemption {
                identity: identity.to_string(),
                reason: reason.to_string(),
                active: true,
                toggle_count: 1,
            });
            None
        }
    }

    /// 查询豁免（O(屏数) 线性，屏数量级小；只认 active 态）。
    pub fn is_exempt(&self, identity: &str) -> bool {
        self.entries.iter().any(|e| e.identity == identity && e.active)
    }

    /// 当前生效豁免的身份清单（联动派生的单一真源）。
    pub fn exempt_identities(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| e.active)
            .map(|e| e.identity.clone())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 五、主流程编排（联动 + 采样 + 豁免一 tick）
// ---------------------------------------------------------------------------

/// 同步引擎（一 tick 编排三件；逻辑时钟零墙钟）。
#[derive(Clone, Debug, Default)]
pub struct SyncEngine {
    /// 监测流（段二）。
    pub monitor: DriftMonitor,
    /// 豁免登记簿（段三）。
    pub book: ExemptionBook,
    /// 最近一次联动的重算屏数。
    pub last_recalced: u32,
    /// 已编排 tick 数。
    pub ticks: u32,
}

impl SyncEngine {
    /// 一 tick：联动派生（豁免屏跳过）→ 副屏采样对拍 → 三通道告警。
    ///
    /// 两个读数源**必须分立**：`current_gamma` 是各屏登记生效值
    /// （联动派生对拍——决定 InSync/重算）；`sampled_gamma` 是本 tick
    /// 物理采样读数（漂移对拍——决定告警）。共用一个源则 InSync 屏
    /// 漂移恒为零、告警路径不可达（恒假）。联动 O(屏数)；每副屏采样
    /// O(1)；告警 O(1)（超阈才立案）。返回本 tick 产出的告警清单。
    pub fn sync_tick(
        &mut self,
        cfg: &SyncConfig,
        current_gamma: &dyn Fn(&str) -> Option<i64>,
        sampled_gamma: &dyn Fn(&str) -> Option<i64>,
        tick: u32,
    ) -> Result<Vec<DriftAlert>, &'static str> {
        self.ticks = self.ticks.saturating_add(1);
        // 段一：联动派生（豁免集合只取 active 态——单一真源）。
        let exempt_ids: Vec<String> = self.book.exempt_identities();
        let outcomes = derive_all(cfg, &exempt_ids, current_gamma)?;
        let recalced = outcomes
            .iter()
            .filter(|(_, o)| matches!(o, DeriveOutcome::Recalced { .. }))
            .count() as u32;
        self.last_recalced = recalced;
        // 段二：副屏逐个采样对拍（重算后的屏以主屏基准为准——重算即对齐）。
        let mut alerts = Vec::new();
        for (identity, outcome) in &outcomes {
            if matches!(outcome, DeriveOutcome::Exempted) {
                continue; // 豁免屏不采样不告警（独立校准合法）
            }
            if matches!(outcome, DeriveOutcome::Recalced { .. }) {
                continue; // 刚重算对齐，本 tick 无漂移可言
            }
            if let Some(g) = sampled_gamma(identity) {
                if let Some(drift) = self.monitor.sample(identity, g, cfg.gamma_per_mille, tick) {
                    alerts.push(drift_alert(identity, drift));
                }
            }
        }
        Ok(alerts)
    }

    /// 读屏摘要（同步状态可查——锚点无障碍口径延伸）。
    pub fn screen_summary(&self, cfg: &SyncConfig) -> String {
        format!(
            "色彩同步：主屏 {}（gamma {}），副屏 {} 台（豁免 {} 台），采样 {} 点，告警 {} 次",
            cfg.primary,
            cfg.gamma_per_mille,
            cfg.secondaries.len(),
            self.book.entries.len(),
            self.monitor.stream.len(),
            self.monitor.alerts
        )
    }
}
