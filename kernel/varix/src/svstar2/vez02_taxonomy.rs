//! VE-F5202 · 特效库定位与分类（VE-Z 域 · 特效 · 四域分类册与定位三律）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5202`
//!
//! 锚点原文：「特效库定位与分类——特效分类四域：战斗反馈（打击感类）/
//! 环境氛围（天气昼夜类）/叙事演出（剧情演出类）/界面反馈（UI 动效类）；
//! 定位三律：语义律（每个特效声明它表达什么——无语义特效不入库）/节制律
//! （特效密度有上限——过犹不及）/可关律（所有特效可整体或单独关闭——晕动症
//! 与低配友好）。数据结构：四域分类册；三律声明卡；语义标注表。错误路径与
//! 降级矩阵：无语义特效→评审拒绝；密度超限→告警+收敛建议；不可关特效→
//! 断言拦截。性能逐项分解：分类 O(特效数)；告警 O(1)；拦截 O(1)。跨批
//! 对接点：F4508 闪烁保护衔接上游；F5201 总架构下游；F5220 双签闸。无障碍
//! 与隐私：可关律即无障碍承诺（域本色核心）；特效清单读屏可达。判据：四域
//! 分类、语义律、节制律、可关律、判据。」
//!
//! # 一、四域不是四种标签，是**四本册子**
//!
//! 分类不是给特效贴字符串——贴标签的分法，同一特效会被贴进多本册子，
//! 「它属于哪域」就变成可吵的问题。[`DomainCatalog`] 每域一本独立桶册，
//! 一个特效**恰好落一域**（[`classify`] 的归属判定由 [`VfxEntry::class`]
//! 与域的关键词面精确映射），桶间互斥可数——四域分类才可审计。
//!
//! # 二、三律是**闸**不是建议
//!
//! - 语义律：[`Semantic::Undeclared`] 的特效在 [`classify`] 即拒
//!   （评审拒绝，码 [`E_VFX2_NO_SEMANTIC`]）——接 F5201 的「无语义不入册」。
//! - 节制律：单域密度超 [`MAX_DENSITY_PER_DOMAIN`] → **告警 + 收敛建议**
//!   （码 [`E_VFX2_DENSITY`]；告警是 O(1) 计数器不是重扫）。
//! - 可关律：不可关特效 → **断言拦截**（码 [`E_VFX2_NOT_CLOSABLE`]）；
//!   主开关 [`MasterSwitch::Off`] 一次关停全部——晕动症用户等的不是
//!   逐个关二十个开关。
//!
//! # 三、可关律即无障碍承诺：清单读屏可达
//!
//! [`screen_listing`] 把四域册 + 可关状态渲染成固定行式清单——特效清单
//! 不是内部账本，是无障碍承诺的**可核对面**：读屏用户能听到「有哪些
//! 特效、各属哪域、能否关闭」，可关律才算兑现。
//!
//! # 四、跨批对接
//!
//! 上游 F4508 闪烁保护：[`classify`] 对闪烁类特效在保护开启时**强制
//! 标记可关**（保护语义前向，见 [`FlickerGuard`]）。下游 F5201：消费
//! [`VfxEntry`]。F5220 双签闸：消费本册的 [`ClassificationLedger`]。

use alloc::format;
use alloc::string::String;

use super::vez01_vfxarch::{Semantic, VfxEntry};

// ---------------------------------------------------------------------------
// 错误契约：三律三码 + 归属一码（独占 0x52 细分段）
// ---------------------------------------------------------------------------

/// 语义律：无语义特效（评审拒绝）。
pub const E_VFX2_NO_SEMANTIC: u16 = 0x5200;
/// 节制律：域密度超限（告警 + 收敛建议）。
pub const E_VFX2_DENSITY: u16 = 0x5201;
/// 可关律：不可关特效（断言拦截）。
pub const E_VFX2_NOT_CLOSABLE: u16 = 0x5202;
/// 归属：类别映射不到任何域（分类册外类别）。
pub const E_VFX2_NO_DOMAIN: u16 = 0x5203;

// ---------------------------------------------------------------------------
// 四域封闭枚举
// ---------------------------------------------------------------------------

/// 特效四域（封闭全集；锚点判据「四域分类」的分类面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    /// 战斗反馈（打击感类）。
    Combat,
    /// 环境氛围（天气昼夜类）。
    Ambient,
    /// 叙事演出（剧情演出类）。
    Narrative,
    /// 界面反馈（UI 动效类）。
    Interface,
}

impl Domain {
    /// 全枚举（顺序即下标）。
    pub const ALL: [Domain; 4] =
        [Domain::Combat, Domain::Ambient, Domain::Narrative, Domain::Interface];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            Domain::Combat => 0,
            Domain::Ambient => 1,
            Domain::Narrative => 2,
            Domain::Interface => 3,
        }
    }

    /// 下标 → 枚举（越界 None——封闭全集不静默兜底）。
    pub const fn of_ordinal(i: usize) -> Option<Domain> {
        match i {
            0 => Some(Domain::Combat),
            1 => Some(Domain::Ambient),
            2 => Some(Domain::Narrative),
            3 => Some(Domain::Interface),
            _ => None,
        }
    }

    /// 域名（读屏用）。
    pub const fn label(self) -> &'static str {
        match self {
            Domain::Combat => "战斗反馈",
            Domain::Ambient => "环境氛围",
            Domain::Narrative => "叙事演出",
            Domain::Interface => "界面反馈",
        }
    }
}

/// 类别关键词 → 域映射（分类 O(1)：匹配域关键词面即归属）。
///
/// 类别字符串由 F5201 `Subsystem::exclusive_class()` 产出；此处按**前缀
/// 面**映射（类别是封闭集，前缀面即全表面）。
pub fn domain_of_class(class: &str) -> Option<Domain> {
    if class.starts_with("打击") || class.starts_with("命中") {
        Some(Domain::Combat)
    } else if class.starts_with("天气")
        || class.starts_with("昼夜")
        || class.starts_with("环境")
        || class.starts_with("闪烁")
    {
        Some(Domain::Ambient)
    } else if class.starts_with("剧情") || class.starts_with("过场") {
        Some(Domain::Narrative)
    } else if class.starts_with("UI") || class.starts_with("界面") {
        Some(Domain::Interface)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 三律声明卡
// ---------------------------------------------------------------------------

/// 三律声明卡：每个特效入库前必须随附的三律自述。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LawCard {
    /// 特效名（与登记名一致）。
    pub name: String,
    /// 语义声明（None = 未声明 = 语义律拒绝）。
    pub semantic: Option<String>,
    /// 是否接受密度上限约束（false = 声明豁免 = 节制律拒绝）。
    pub density_capped: bool,
    /// 是否可关（false = 不可关 = 可关律拦截）。
    pub closable: bool,
}

impl LawCard {
    /// 构造声明卡。
    pub fn new(name: &str, semantic: Option<&str>, density_capped: bool, closable: bool) -> LawCard {
        LawCard {
            name: String::from(name),
            semantic: semantic.map(String::from),
            density_capped,
            closable,
        }
    }
}

// ---------------------------------------------------------------------------
// 语义标注表
// ---------------------------------------------------------------------------

/// 语义标注表：特效名 → 语义描述。
///
/// 「查无此条」与「描述为空串」是两回事：查无（None）= 根本没标注 =
/// 语义律不过；空串条目 = 标注了但描述空白（同样是语义律不过——
/// 空描述不表达任何东西）。二者都拒，但来源可分。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SemanticTable {
    entries: Vec<(String, String)>,
}

impl SemanticTable {
    /// 空表。
    pub fn new() -> SemanticTable {
        SemanticTable { entries: Vec::new() }
    }

    /// 登记语义（O(n) 表内查重，特效数受密度上限约束故规模有限）。
    pub fn declare(&mut self, name: &str, semantic: &str) {
        for e in self.entries.iter_mut() {
            if e.0 == name {
                e.1 = String::from(semantic);
                return;
            }
        }
        self.entries.push((String::from(name), String::from(semantic)));
    }

    /// 查询：None = 未标注；Some("") = 标注了但空白。
    pub fn lookup(&self, name: &str) -> Option<&str> {
        for e in self.entries.iter() {
            if e.0 == name {
                return Some(e.1.as_str());
            }
        }
        None
    }

    /// 已标注条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 表是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 闪烁保护（上游 F4508 衔接）
// ---------------------------------------------------------------------------

/// 上游闪烁保护状态（F4508 语义前向：保护开启时闪烁类特效强制可关）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlickerGuard {
    /// 保护未开启（上游未声明）。
    Inactive,
    /// 保护开启（闪烁类特效强制可关且默认关）。
    Active,
}

/// 闪烁类类别面（命中即受 F4508 强制可关约束）。
fn is_flicker_class(class: &str) -> bool {
    class.contains("闪烁") || class.contains("频闪")
}

// ---------------------------------------------------------------------------
// 分类引擎
// ---------------------------------------------------------------------------

/// 单域密度上限（节制律；域内容量——过犹不及的量化线）。
pub const MAX_DENSITY_PER_DOMAIN: usize = 16;

/// 主开关（可关律的「整体关闭」面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MasterSwitch {
    /// 特效开启（按各特效开关逐个生效）。
    On,
    /// 整体关停（晕动症/低配一键关停——读屏清单如实呈现）。
    Off,
}

/// 分类结局。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    /// 已入册（入哪域 + 密度告警与否）。
    Filed { domain: Domain, density_warned: bool },
    /// 评审拒绝（三律闸各专属码）。
    Rejected(u16),
}

/// 分类引擎 + 四域分类册。
#[derive(Clone, Debug)]
pub struct DomainCatalog {
    /// 四域桶册（下标即 [`Domain::ordinal`]）。
    buckets: [Vec<String>; 4],
    /// 语义标注表。
    pub semantics: SemanticTable,
    /// 主开关。
    pub master: MasterSwitch,
    /// 上游闪烁保护。
    pub flicker: FlickerGuard,
    /// 密度告警计数（O(1) 告警面）。
    pub density_warnings: u32,
    /// 不可关拦截计数（O(1) 拦截面）。
    pub closable_blocks: u32,
    /// 评审拒绝计数（语义律 + 归属失败）。
    pub rejected: u32,
}

impl DomainCatalog {
    /// 空册（主开关开、闪烁保护未开启）。
    pub fn new() -> DomainCatalog {
        DomainCatalog {
            buckets: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            semantics: SemanticTable::new(),
            master: MasterSwitch::On,
            flicker: FlickerGuard::Inactive,
            density_warnings: 0,
            closable_blocks: 0,
            rejected: 0,
        }
    }

    /// 某域当前密度（桶长）。
    pub fn density(&self, d: Domain) -> usize {
        self.buckets[d.ordinal()].len()
    }

    /// 全册特效数。
    pub fn total(&self) -> usize {
        let mut n = 0usize;
        for b in self.buckets.iter() {
            n += b.len();
        }
        n
    }

    /// 某特效是否在册。
    pub fn contains(&self, name: &str) -> bool {
        for b in self.buckets.iter() {
            for n in b.iter() {
                if n == name {
                    return true;
                }
            }
        }
        false
    }

    /// **classify**：三律闸 + 归属入册。
    ///
    /// 闸序（每一道都专属码，不合并归因）：
    /// 1. 可关律（断言拦截 O(1)）：不可关即拦——先于语义律，因为「不可关」
    ///    是无障碍红线，红线不过连评审都不必进；
    /// 2. 语义律（评审拒绝）：声明卡未声明或语义表查无/空白；
    /// 3. 归属（册外类别）：类别映射不到域；
    /// 4. 节制律（告警 + 收敛建议）：入册后密度超限只告警不拒——
    ///    密度是体验问题不是正确性问题，告警留痕即可。
    pub fn classify(&mut self, entry: &VfxEntry, card: &LawCard) -> Placement {
        // 闸 1：可关律（断言拦截）。
        let forced_closable =
            self.flicker == FlickerGuard::Active && is_flicker_class(entry.class);
        if !card.closable && !forced_closable {
            self.closable_blocks += 1;
            return Placement::Rejected(E_VFX2_NOT_CLOSABLE);
        }
        // 闸 2：语义律（评审拒绝）——声明卡与语义表双源对账。
        let declared = match (&card.semantic, self.semantics.lookup(&entry.name)) {
            (Some(v), None) => !v.is_empty(),
            (Some(cv), Some(tv)) => !cv.is_empty() && !tv.is_empty(),
            (None, Some(tv)) => !tv.is_empty(),
            (None, None) => false,
        };
        if matches!(entry.semantic, Semantic::Undeclared) || !declared {
            self.rejected += 1;
            return Placement::Rejected(E_VFX2_NO_SEMANTIC);
        }
        // 闸 3：归属（册外类别拒）。
        let d = match domain_of_class(entry.class) {
            Some(d) => d,
            None => {
                self.rejected += 1;
                return Placement::Rejected(E_VFX2_NO_DOMAIN);
            }
        };
        // 入册 + 闸 4：节制律（告警不拒）。
        let idx = d.ordinal();
        self.buckets[idx].push(String::from(&entry.name));
        let warned = self.buckets[idx].len() > MAX_DENSITY_PER_DOMAIN;
        if warned {
            self.density_warnings += 1;
        }
        Placement::Filed { domain: d, density_warned: warned }
    }

    /// 收敛建议（密度告警随附；非空即有效——锚点「告警+收敛建议」）。
    pub fn convergence_advice(&self, d: Domain) -> String {
        format!(
            "域「{}」密度 {} 超上限 {}：建议合并同类特效或降低触发频率",
            d.label(),
            self.buckets[d.ordinal()].len(),
            MAX_DENSITY_PER_DOMAIN
        )
    }

    /// 读屏清单（可关律的无障碍可核对面：四域册 + 主开关 + 各域可关性）。
    ///
    /// 行数固定可预算：表头 1 行 + 域行 4 行 + 尾行 1 行 = 6 行。
    pub fn screen_listing(&self) -> [String; 6] {
        let head = match self.master {
            MasterSwitch::On => String::from("特效清单（整体可关：是）"),
            MasterSwitch::Off => String::from("特效清单（当前整体已关停）"),
        };
        let mut rows: [String; 6] = [
            head,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ];
        let mut i = 0usize;
        while i < 4 {
            let d = Domain::of_ordinal(i).unwrap_or(Domain::Combat);
            rows[i + 1] = format!(
                "{}：{} 项（密度上限 {}）",
                d.label(),
                self.buckets[i].len(),
                MAX_DENSITY_PER_DOMAIN
            );
            i += 1;
        }
        rows[5] = format!(
            "共 {} 项；密度告警 {} 次；不可关拦截 {} 次",
            self.total(),
            self.density_warnings,
            self.closable_blocks
        );
        rows
    }
}

// ---------------------------------------------------------------------------
// 双签闸消费面（F5220 前向声明）
// ---------------------------------------------------------------------------

/// 分类总账（F5220 双签闸消费的移交面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassificationLedger {
    /// 四域计数（下标即 [`Domain::ordinal`]）。
    pub per_domain: [usize; 4],
    /// 三律执行计数：语义拒绝 / 密度告警 / 不可关拦截。
    pub law_counters: [u32; 3],
    /// 主开关状态（线值：0=关 1=开）。
    pub master_wire: u8,
}

impl DomainCatalog {
    /// 出具分类总账（O(1)：四桶长 + 三计数器，不重扫册）。
    pub fn ledger(&self) -> ClassificationLedger {
        ClassificationLedger {
            per_domain: [
                self.buckets[0].len(),
                self.buckets[1].len(),
                self.buckets[2].len(),
                self.buckets[3].len(),
            ],
            law_counters: [self.rejected, self.density_warnings, self.closable_blocks],
            master_wire: match self.master {
                MasterSwitch::On => 1,
                MasterSwitch::Off => 0,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// 编译期闸
// ---------------------------------------------------------------------------

const _: () = {
    assert!(MAX_DENSITY_PER_DOMAIN == 16);
    assert!(E_VFX2_NO_SEMANTIC & 0xFF00 == 0x5200);
    assert!(E_VFX2_DENSITY & 0xFF00 == 0x5200);
    assert!(E_VFX2_NOT_CLOSABLE & 0xFF00 == 0x5200);
    assert!(E_VFX2_NO_DOMAIN & 0xFF00 == 0x5200);
    assert!(
        E_VFX2_NO_SEMANTIC != E_VFX2_DENSITY
            && E_VFX2_DENSITY != E_VFX2_NOT_CLOSABLE
            && E_VFX2_NOT_CLOSABLE != E_VFX2_NO_DOMAIN
    );
};
