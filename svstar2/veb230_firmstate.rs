//! VE-F0230 Intel 固件接口只读状态
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0230`
//!
//! 职责：GuC/HuC 固件加载状态与运行统计**只读采集呈现**；固件加载失败按
//! 能力降级到 EXECLISTS 直通模式并通知（F0103 三要素）；状态读取失败标记
//! 缺测；统计异常**如实呈现不修数**。上游 F0221（代际口径）/F0224（提交
//! 通路）；下游 F0235 缺陷表含固件相关规避项。
//!
//! ## 要点一：只读红线是**结构性**的，不是口头承诺
//!
//! 本模块对固件接口的全部访问都经由 [`FwRegBank`]，且采集入口签名是
//! `collect(&mut self, bank: &FwRegBank)`——银行按**共享引用**传入，类型
//! 上不存在任何写路径（无 `&mut`、无内部可变性、无 `Cell`）。判据层用
//! 「采集前后银行摘要逐位不变」把红线变成**可运行断言**：任何写入都会
//! 改变摘要而被当场抓住。
//!
//! ## 要点二：加载态是闭集，解码走查表不走猜值
//!
//! 状态寄存器高半字是接口存在魔数，低位才是加载态与认证位。魔数不符或
//! 读到全 1（读取失败）不是「未知状态」而是**缺测**——缺测与「未加载」
//! 是两种病：前者是采集面的失败，后者是固件面的结论，混报等于撒谎。
//!
//! ## 要点三：降级只由 GuC 失败触发，HuC 失败只标注
//!
//! GuC 承担提交调度，其加载失败才降级提交通路（EXECLISTS 直通，与
//! F0224 的 ELSP 队头成对直入同一语义）；HuC 服务媒体/功耗特性，其失败
//! 不影响提交语义——把 HuC 失败也降级提交是把降级当装饰。降级切换 O(1)
//! （两次布尔赋值），通知**边沿触发**只发一次，不随周期刷屏。
//!
//! ## 要点四：缺测显性标记，绝不编值补位
//!
//! 状态/统计任一读取失败，对应通道标记缺测并携带诊断码；快照保留缺测
//! 状态供呈现层如实播报。**不允许**用 0 或上次值静默补位——补出来的
//! 「正常」比缺测更危险。
//!
//! ## 要点五：统计异常如实呈现不修数
//!
//! 计数器倒退/越界（errors > submissions 等）记录异常**保留原值**：
//! 不钳制、不丢弃、不重置。修数会让漂移检测（下游 F0235 参考）失去
//! 唯一的事实来源。异常容量有界，满即如实标记截断。
//!
//! ## 要点六：诊断码独占 0x3Dxx 段
//!
//! 与 F0228（0x3Bxx）/F0229（0x3Cxx）/F0224（0x34xx）互不重叠；每码
//! 专属 reason；码段判据用 `!=` 防自判死。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::veb12_recovery::{DegradeNotice, Severity, NOTICE_LINK_VERSION};
use super::veb21_ident::GenTier;

// ---------------------------------------------------------------------------
// 一、常量（性能逐项：采集 O(1) 周期拉取、降级切换 O(1)、只读零写路径）
// ---------------------------------------------------------------------------

/// B2 域标签（诊断桥接来源标注；B2 = Intel 组，见 mod.rs 域地图）。
pub const DOMAIN_TAG_B2: u8 = 2;

/// 状态寄存器高半字魔数：证明该寄存器确实是固件状态接口。
pub const FW_STATUS_MAGIC: u32 = 0xC0DE_0000;

/// 读取失败特征值（总线全 1）——这是缺测，不是「未加载」。
pub const READ_FAIL: u32 = 0xFFFF_FFFF;

/// 每通道统计寄存器字数（提交×完成×错误×序号）。
pub const STAT_WORDS: usize = 4;

/// 异常清单容量上界（满即截断并如实标记，不静默丢弃也不无限增长）。
///
/// 取 3：单周期两通道可产出 4 条异常（各 2 条），呈现层一屏能说完的
/// 事实就直说，多出的如实标记截断——截断本身也是要呈现的事实。
pub const MAX_ANOMALIES: usize = 3;

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x3Dxx 段）
// ---------------------------------------------------------------------------

/// F0230 诊断码。独占 `0x3Dxx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FwCode(pub u16);

impl FwCode {
    /// GuC 状态寄存器缺测（读取失败/魔数不符）。
    pub const GUC_STATUS_MISS: FwCode = FwCode(0x3D01);
    /// HuC 状态寄存器缺测。
    pub const HUC_STATUS_MISS: FwCode = FwCode(0x3D02);
    /// 统计寄存器缺测（任一统计字读取失败按整组缺测）。
    pub const STATS_MISS: FwCode = FwCode(0x3D03);
    /// 统计序列异常（倒退/越界/回绕——如实呈现标记，可观测非致命）。
    pub const STATS_ANOMALY: FwCode = FwCode(0x3D04);
    /// 状态位组非法（低两位出现保留组合）。
    pub const STATE_INVALID: FwCode = FwCode(0x3D05);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            FwCode::GUC_STATUS_MISS => "GuC 状态寄存器缺测：读取失败或接口魔数不符".into(),
            FwCode::HUC_STATUS_MISS => "HuC 状态寄存器缺测：读取失败或接口魔数不符".into(),
            FwCode::STATS_MISS => "固件运行统计缺测：统计寄存器读取失败，不补位不编值".into(),
            FwCode::STATS_ANOMALY => "固件统计异常：计数序列倒退或越界，原值保留如实呈现".into(),
            FwCode::STATE_INVALID => "固件加载态位组非法：保留位组合越界，按缺测处理".into(),
            FwCode(_) => "未知固件接口诊断码".into(),
        }
    }

    /// 桥接到内核诊断命名空间：`(域标签, wire 码)` 二元组。
    ///
    /// 桥接是**映射不是改写**——wire 码原样上桥，域标签只标注来源域，
    /// 保证下游按桥接对账时码值仍是本域独占段的值。
    pub const fn bridge(self) -> (u8, u16) {
        (DOMAIN_TAG_B2, self.0)
    }
}

// ---------------------------------------------------------------------------
// 三、固件种类与加载态（闭集）
// ---------------------------------------------------------------------------

/// 固件种类（GuC 提交调度 / HuC 媒体功耗）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FwKind {
    /// GuC：图形微控制器，承担提交调度——加载失败降级提交通路。
    Guc,
    /// HuC：heuristics 微控制器，服务媒体/功耗——失败只标注不降级提交。
    Huc,
}

impl FwKind {
    /// 人话标签（呈现层直接可读）。
    pub fn label(self) -> String {
        match self {
            FwKind::Guc => "GuC 提交固件".to_string(),
            FwKind::Huc => "HuC 媒体固件".to_string(),
        }
    }

    /// 该固件状态缺测对应的诊断码。
    pub fn status_miss_code(self) -> FwCode {
        match self {
            FwKind::Guc => FwCode::GUC_STATUS_MISS,
            FwKind::Huc => FwCode::HUC_STATUS_MISS,
        }
    }
}

/// 固件加载态（闭集四态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    /// 未加载（采集正常得到的结论，不是缺测）。
    NotLoaded,
    /// 加载中。
    Loading,
    /// 已加载且认证通过。
    Loaded,
    /// 加载失败或认证失败（GuC 到此态触发降级）。
    Failed,
}

/// 从状态寄存器解码加载态。
///
/// 边界防护：全 1 = 读取失败（缺测）；魔数不符 = 接口不可信（缺测）；
/// 保留位组 = 越界（[`FwCode::STATE_INVALID`]）。三种异常都**不猜值**。
pub fn decode_load_state(raw: u32) -> Result<LoadState, FwCode> {
    if raw == READ_FAIL {
        return Err(FwCode::GUC_STATUS_MISS); // 缺测码由通道层按种类改写
    }
    if (raw & 0xFFFF_0000) != FW_STATUS_MAGIC {
        return Err(FwCode::GUC_STATUS_MISS);
    }
    match raw & 0b11 {
        0b00 => Ok(LoadState::NotLoaded),
        0b01 => Ok(LoadState::Loading),
        0b10 => {
            // 已加载还要求认证位（bit 4）成立；未认证按失败处理。
            if raw & (1 << 4) != 0 {
                Ok(LoadState::Loaded)
            } else {
                Ok(LoadState::Failed)
            }
        }
        0b11 => Ok(LoadState::Failed),
        _ => Err(FwCode::STATE_INVALID),
    }
}

// ---------------------------------------------------------------------------
// 四、运行统计与异常（如实呈现不修数）
// ---------------------------------------------------------------------------

/// 固件运行统计（提交×完成×错误×序号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FwStats {
    /// 固件侧受理的提交数。
    pub submissions: u32,
    /// 固件侧完成的提交数。
    pub completions: u32,
    /// 固件侧报告的错误数。
    pub errors: u32,
    /// 上下文序号（回绕检测参考）。
    pub seqno: u32,
}

impl FwStats {
    /// 组内一致性自检（不动数值，只判定是否异常）。
    ///
    /// 完成 > 提交或错误 > 提交都是序列异常——**判定归判定，数值原样
    /// 保留**：这里返回异常标记，绝不顺手把越界值改成「合理」值。
    pub fn is_inconsistent(&self) -> bool {
        self.completions > self.submissions || self.errors > self.submissions
    }
}

/// 统计异常记录（原值保留——下游 F0235 漂移检测的事实来源）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatAnomaly {
    /// 异常通道的固件种类。
    pub kind: FwKind,
    /// 诊断码（恒为 [`FwCode::STATS_ANOMALY`] 或回绕标注）。
    pub code: FwCode,
    /// **观测到的原始值**（不钳制不修正——修数即销毁证据）。
    pub observed: u32,
    /// 异常类别人话说明。
    pub detail: String,
}

// ---------------------------------------------------------------------------
// 五、只读硬件窗口（FwRegBank——零写路径的结构保证）
// ---------------------------------------------------------------------------

/// 固件接口寄存器银行：采集面看到的「硬件读数」注入窗口。
///
/// **本类型是只读红线载体**：没有 `pub fn` 会修改任何字段，采集入口按
/// `&FwRegBank` 共享引用消费。摘要（FNV-1a 逐字滚动）供判据层验证
/// 「采集前后银行逐位不变」——红线因此可运行断言而非口头承诺。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FwRegBank {
    /// GuC 状态寄存器。
    pub guc_status: u32,
    /// HuC 状态寄存器。
    pub huc_status: u32,
    /// GuC 统计寄存器组（提交/完成/错误/序号）。
    pub guc_stats: [u32; STAT_WORDS],
    /// HuC 统计寄存器组。
    pub huc_stats: [u32; STAT_WORDS],
}

impl FwRegBank {
    /// 银行摘要（FNV-1a，逐字滚动，判据侧同口径独立重算对拍）。
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for w in [
            self.guc_status,
            self.huc_status,
            self.guc_stats[0],
            self.guc_stats[1],
            self.guc_stats[2],
            self.guc_stats[3],
            self.huc_stats[0],
            self.huc_stats[1],
            self.huc_stats[2],
            self.huc_stats[3],
        ] {
            h ^= w;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 某通道的统计组读取：任一字为全 1 视为整组缺测。
    fn stats_of(&self, kind: FwKind) -> Result<FwStats, FwCode> {
        let words = match kind {
            FwKind::Guc => &self.guc_stats,
            FwKind::Huc => &self.huc_stats,
        };
        for w in words {
            if *w == READ_FAIL {
                return Err(FwCode::STATS_MISS);
            }
        }
        Ok(FwStats {
            submissions: words[0],
            completions: words[1],
            errors: words[2],
            seqno: words[3],
        })
    }
}

// ---------------------------------------------------------------------------
// 六、通道与快照（数据结构：固件状态快照＝加载态×统计；降级模式标记）
// ---------------------------------------------------------------------------

/// 单固件通道视图（加载态 × 统计 × 缺测标记）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FwChannel {
    /// 固件种类。
    pub kind: FwKind,
    /// 加载态（None = 状态缺测，绝不编造）。
    pub load: Option<LoadState>,
    /// 运行统计（None = 统计缺测）。
    pub stats: Option<FwStats>,
    /// 缺测诊断码（状态或统计任一缺测即记录，缺测显性）。
    pub missing: Option<FwCode>,
}

impl FwChannel {
    /// 本通道是否缺测（呈现层据此播报缺测而非展示编造的「正常」）。
    pub fn is_missing(&self) -> bool {
        self.missing.is_some()
    }

    /// 呈现文本：加载态×统计×缺测一次说全（如实呈现不修饰）。
    pub fn present(&self) -> String {
        let state = match self.load {
            Some(s) => match s {
                LoadState::NotLoaded => "未加载",
                LoadState::Loading => "加载中",
                LoadState::Loaded => "已加载",
                LoadState::Failed => "加载失败",
            },
            None => "状态缺测",
        };
        let stats = match self.stats {
            Some(s) => format!(
                "提交{} 完成{} 错误{}",
                s.submissions, s.completions, s.errors
            ),
            None => "统计缺测".to_string(),
        };
        if self.is_missing() {
            format!("{}：{}（{}）——存在缺测项", self.kind.label(), state, stats)
        } else {
            format!("{}：{}；{}", self.kind.label(), state, stats)
        }
    }
}

/// 降级模式标记（锚点数据结构第二项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradeMode {
    /// 正常态：GuC 提交（固件参与调度）。
    GuSubmit,
    /// 降级态：EXECLISTS 直通模式（与 F0224 ELSP 队头成对直入同语义）。
    ExeclistsDirect,
}

impl DegradeMode {
    /// 人话标签。
    pub fn label(self) -> String {
        match self {
            DegradeMode::GuSubmit => "GuC 固件提交模式".to_string(),
            DegradeMode::ExeclistsDirect => "EXECLISTS 直通模式（GuC 降级）".to_string(),
        }
    }
}

/// 固件状态快照（一次采集的完整结论，只读呈现给下游 F0235）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmSnapshot {
    /// GuC 通道。
    pub guc: FwChannel,
    /// HuC 通道。
    pub huc: FwChannel,
    /// 当前降级模式标记。
    pub mode: DegradeMode,
    /// 是否处于降级（GuC 失败降级到直通）。
    pub degraded: bool,
    /// 统计异常清单（原值保留）。
    pub anomalies: Vec<StatAnomaly>,
    /// 异常清单是否被容量截断（截断也要如实）。
    pub anomalies_truncated: bool,
    /// 代际口径标签（上游 F0221，供 F0235 缺陷表按代际定位）。
    pub gen_label: String,
    /// 采集周期号。
    pub tick: u64,
}

impl FirmSnapshot {
    /// 任一通道缺测（呈现层据此标注快照级缺测）。
    pub fn has_missing(&self) -> bool {
        self.guc.is_missing() || self.huc.is_missing()
    }

    /// 快照级呈现文本（两通道 + 降级模式，不修饰不省略）。
    pub fn present(&self) -> String {
        let degrade = if self.degraded {
            format!("已降级：{}", self.mode.label())
        } else {
            "正常".to_string()
        };
        format!("固件状态（周期 {}）｜{}｜{}｜{}", self.tick, self.guc.present(), self.huc.present(), degrade)
    }
}

// ---------------------------------------------------------------------------
// 七、只读采集器（FirmReader——核心逻辑编排）
// ---------------------------------------------------------------------------

/// GuC 降级通知的 F0103 类别标签（借用本域码段前缀，可追溯）。
pub const DEGRADE_CATEGORY: u32 = 0x3D00;

/// 固件接口只读采集器。
///
/// 持有跨周期的统计基线（用于异常判定）与降级锁存状态。对 [`FwRegBank`]
/// 只有读——红线的结构保证见模块头要点一。
#[derive(Debug, Clone)]
pub struct FirmReader {
    cycle: u64,
    prev_guc: Option<FwStats>,
    prev_huc: Option<FwStats>,
    degraded: bool,
    switches: u32,
    notices: Vec<DegradeNotice>,
}

impl FirmReader {
    /// 新采集器（周期从 1 起；降级判定每周期重算，切换边沿计数）。
    pub fn new() -> FirmReader {
        FirmReader {
            cycle: 0,
            prev_guc: None,
            prev_huc: None,
            degraded: false,
            switches: 0,
            notices: Vec::new(),
        }
    }

    /// 当前是否降级。
    pub fn degraded(&self) -> bool {
        self.degraded
    }

    /// 降级切换次数（可观测——频繁切换本身就是缺陷信号）。
    pub fn switches(&self) -> u32 {
        self.switches
    }

    /// 拉走累计的降级通知（交给 F0103 限频合并——本层不做限频，源头
    /// 限频丢事件连回溯原料都没有，同 F0226 口径）。
    pub fn take_notices(&mut self) -> Vec<DegradeNotice> {
        core::mem::take(&mut self.notices)
    }

    /// 只读采集一次（O(1)：固定寄存器数 × 固定比较次数，与任何规模
    /// 无关）。快照缺测/异常/降级全部如实标记。
    pub fn collect(&mut self, bank: &FwRegBank, gen: GenTier) -> FirmSnapshot {
        self.cycle += 1;
        let mut anomalies: Vec<StatAnomaly> = Vec::new();

        // —— 两通道加载态解码（缺测不改写为「未加载」）——
        let guc_load = decode_load_state(bank.guc_status)
            .map_err(|_| FwKind::Guc.status_miss_code())
            .ok();
        let guc_missing = if guc_load.is_none() {
            Some(FwKind::Guc.status_miss_code())
        } else {
            None
        };
        let huc_load = decode_load_state(bank.huc_status)
            .map_err(|_| FwKind::Huc.status_miss_code())
            .ok();
        let huc_missing = if huc_load.is_none() {
            Some(FwKind::Huc.status_miss_code())
        } else {
            None
        };

        // —— 两通道统计采集 + 异常判定（原值保留）——
        let guc_stats = self.take_stats(FwKind::Guc, bank, &mut anomalies);
        let huc_stats = self.take_stats(FwKind::Huc, bank, &mut anomalies);

        // —— 统计缺测也是缺测：状态正常但统计读不到，通道照样标缺测 ——
        let guc_missing = guc_missing.or({
            if guc_stats.is_none() {
                Some(FwCode::STATS_MISS)
            } else {
                None
            }
        });
        let huc_missing = huc_missing.or({
            if huc_stats.is_none() {
                Some(FwCode::STATS_MISS)
            } else {
                None
            }
        });

        // —— 异常容量截断（如实标记，不静默丢）——
        let mut anomalies_truncated = false;
        if anomalies.len() > MAX_ANOMALIES {
            anomalies.truncate(MAX_ANOMALIES);
            anomalies_truncated = true;
        }

        // —— 降级判定：只看 GuC（要点三）；HuC 失败不牵连提交 ——
        let guc_failed = guc_load == Some(LoadState::Failed);
        let was = self.degraded;
        self.degraded = guc_failed;
        if was != self.degraded {
            self.switches += 1; // 降级切换 O(1)：一次比较一次赋值一次计数
        }
        // 通知边沿触发：只在 false→true 时刻发一次，不随周期刷屏。
        if guc_failed && !was {
            let kind_label = FwKind::Guc.label();
            if let Ok(n) = DegradeNotice::new(
                DEGRADE_CATEGORY | (FwCode::GUC_STATUS_MISS.code() as u32),
                Severity::Queue,
                format!("{}加载失败，提交通路已切换到直通模式", kind_label),
                "GuC 固件未通过加载或认证，固件参与调度不可用".to_string(),
                "图形提交已按 EXECLISTS 直通模式继续运行，性能可能受限；可重启图形会话重试固件加载".to_string(),
                self.cycle,
            ) {
                self.notices.push(n);
            }
        }

        FirmSnapshot {
            guc: FwChannel {
                kind: FwKind::Guc,
                load: guc_load,
                stats: guc_stats,
                missing: guc_missing,
            },
            huc: FwChannel {
                kind: FwKind::Huc,
                load: huc_load,
                stats: huc_stats,
                missing: huc_missing,
            },
            mode: if self.degraded {
                DegradeMode::ExeclistsDirect
            } else {
                DegradeMode::GuSubmit
            },
            degraded: self.degraded,
            anomalies,
            anomalies_truncated,
            gen_label: gen.label(),
            tick: self.cycle,
        }
    }

    /// 单通道统计采集 + 与上周期基线的异常对账（原值保留，判据对拍面）。
    fn take_stats(
        &mut self,
        kind: FwKind,
        bank: &FwRegBank,
        out: &mut Vec<StatAnomaly>,
    ) -> Option<FwStats> {
        match bank.stats_of(kind) {
            Err(_) => None, // 缺测：整组 None，绝不拿旧值补位（要点四）
            Ok(cur) => {
                let prev = match kind {
                    FwKind::Guc => self.prev_guc.replace(cur),
                    FwKind::Huc => self.prev_huc.replace(cur),
                };
                if let Some(p) = prev {
                    // 计数倒退/回绕形态都如实记异常（原值保留）——倒退是
                    // 序列病，回绕是正常溢出形态，二者 detail 区分但都不
                    // 顺手「修数」：清零重排就是销毁漂移检测的事实来源。
                    if cur.submissions < p.submissions {
                        let wrap = Self::looks_like_wrap(p.submissions, cur.submissions);
                        out.push(StatAnomaly {
                            kind,
                            code: FwCode::STATS_ANOMALY,
                            observed: cur.submissions,
                            detail: if wrap {
                                format!(
                                    "{}提交计数回绕：{} -> {}（回绕形态，原值保留）",
                                    kind.label(),
                                    p.submissions,
                                    cur.submissions
                                )
                            } else {
                                format!(
                                    "{}提交计数倒退：{} -> {}（原值保留）",
                                    kind.label(),
                                    p.submissions,
                                    cur.submissions
                                )
                            },
                        });
                    }
                    if cur.is_inconsistent() {
                        out.push(StatAnomaly {
                            kind,
                            code: FwCode::STATS_ANOMALY,
                            observed: cur.errors.max(cur.completions),
                            detail: format!(
                                "{}组内越界：提交{} 完成{} 错误{}（原值保留）",
                                kind.label(), cur.submissions, cur.completions, cur.errors
                            ),
                        });
                    }
                }
                Some(cur)
            }
        }
    }

    /// 回绕形态判定：上周期接近上界且本周期显著小于上周期——按回绕
    /// 记异常（如实），不按「清零修数」处理。
    fn looks_like_wrap(prev: u32, cur: u32) -> bool {
        prev > u32::MAX - 4096 && cur < prev && prev - cur > u32::MAX / 2
    }
}

impl Default for FirmReader {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 八、测试支撑（回归用例与断言）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;

    fn ok_bank() -> FwRegBank {
        FwRegBank {
            guc_status: FW_STATUS_MAGIC | 0b10 | (1 << 4), // 已加载已认证
            huc_status: FW_STATUS_MAGIC | 0b10 | (1 << 4),
            guc_stats: [10, 9, 1, 100],
            huc_stats: [5, 5, 0, 50],
        }
    }

    #[test]
    fn 快照全绿无降级() {
        let mut r = FirmReader::new();
        let s = r.collect(&ok_bank(), GenTier::XeStandard);
        assert!(!s.degraded);
        assert!(!s.has_missing());
        assert!(s.anomalies.is_empty());
    }

    #[test]
    fn guc加载失败降级并出三要素通知() {
        let mut b = ok_bank();
        b.guc_status = FW_STATUS_MAGIC | 0b11; // Failed
        let mut r = FirmReader::new();
        let s = r.collect(&b, GenTier::XeStandard);
        assert!(s.degraded);
        assert_eq!(s.mode, DegradeMode::ExeclistsDirect);
        let ns = r.take_notices();
        assert_eq!(ns.len(), 1);
        assert!(ns[0].has_triplet());
    }

    #[test]
    fn huc失败不降级() {
        let mut b = ok_bank();
        b.huc_status = FW_STATUS_MAGIC | 0b11;
        let mut r = FirmReader::new();
        let s = r.collect(&b, GenTier::XeStandard);
        assert!(!s.degraded);
    }

    #[test]
    fn 缺测显性且不补位() {
        let mut b = ok_bank();
        b.huc_status = READ_FAIL;
        b.huc_stats = [READ_FAIL; STAT_WORDS];
        let mut r = FirmReader::new();
        let s = r.collect(&b, GenTier::XeStandard);
        assert!(s.huc.is_missing());
        assert!(s.huc.load.is_none());
        assert!(s.huc.stats.is_none());
    }

    #[test]
    fn 统计倒退原值保留() {
        let mut r = FirmReader::new();
        let _ = r.collect(&ok_bank(), GenTier::XeStandard);
        let mut b = ok_bank();
        b.guc_stats = [3, 2, 0, 100]; // 10 -> 3 倒退
        let s = r.collect(&b, GenTier::XeStandard);
        assert_eq!(s.anomalies.len(), 1);
        assert_eq!(s.anomalies[0].observed, 3);
    }
}
