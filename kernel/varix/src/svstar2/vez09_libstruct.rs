//! VE-F5209 · 特效库结构设计（VE-Z 域 · 库结构面 · 锚点目标 320 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5209`
//!
//! **判据（锚点原文）**：目录规范、元数据即索引、版本迁移、驳回、判据。
//!
//! **职责定位（锚点原文）**：特效库结构设计——库结构两件：资产组织
//! （特效资产目录结构与命名规范——找得到看得懂）/元数据规范（每特效
//! 元数据（分类/语义/预算/参数引用）——元数据即索引）；库结构版本化
//! 与迁移（结构演进走迁移清单）。
//!
//! **数据结构（锚点原文）**：目录结构规范；元数据规范册；迁移清单。
//!
//! **错误路径与降级矩阵（锚点原文）**：命名违规→驳回整改；元数据缺
//! 失→断言拦截；结构漂移→迁移对拍。
//!
//! **性能逐项分解（锚点原文）**：组织 O(资产数)；拦截 O(1)；对拍 O(1)。
//!
//! **跨批对接点（锚点原文）**：F4405 配置口径参照上游；F5210 遥测消费。
//! F5207 参数 schema 被元数据引用（参数引用面）。
//!
//! **无障碍与隐私（锚点原文）**：找得到看得懂即域本色（核心）；无隐
//! 私面。
//!
//! # 深化说明：库不是文件堆——每个名字都过规范闸，每条元数据都是
//! ## 索引，每次结构演进都走迁移清单
//!
//! 「找得到看得懂」如果只靠约定，三个月后库里就会出现
//! `New_Folder(3)/final_final2.fx`——命名规范必须是**驳回闸**不是建议：
//! 五规则闭集（长度/起头/字符集/分隔/前缀）逐名校验，违规驳回且**必附
//! 整改提示**（[`hint_of`] 按违规形给「应为什么样」——驳回整改是给人
//! 指路不是给人闭门羹，锚点「命名违规→驳回整改」）；「元数据即索引」
//! 落成 BTreeMap 索引册：查元数据 O(log n)，全集对拍缺一拦截计数
//! O(1)——没有元数据的特效就是没有说明书的产品；「结构演进走迁移清
//! 单」落成步序链：from 必须接链尾、to 必须严格递增（断链/倒退
//! [`E_LB_BAD_MIG`]），apply 不跳步，当前版本与清单终点对拍 O(1)
//! （[`MigrationLedger::audit_structure`]）——不齐即结构漂移
//! [`E_LB_DRIFT`]，漂移可见比漂移蔓延便宜得多。
//!
//! ## 深化一：命名规范是五规则闭集——违规形各带专属整改提示
//!
//! 五规则（[`validate_name`]）：①长度 4..=32（过短无信息量过长读不
//! 完）②不以数字开头（类前缀起头是库的入口序）③小写蛇形字符集（大
//! 写/空格/连字符在跨平台路径上各有坑）④禁双下划线/尾下划线（残缺
//! 形）⑤类前缀闭集 fx_p_战斗/fx_e_环境/fx_t_演出/fx_i_界面/fx_s_共
//! 享（F5201 四子系统+F5202 四域同源口径——前缀即域归属，看名知类）。
//! 违规驳回 [`E_LB_NAME`]，[`hint_of`] 按违规形给专属提示（判据侧对拍
//! 提示非空且与违规形对应——提示可执行不是模板句）。
//!
//! ## 深化二：目录结构是闭集映射不是自由生长——前缀→类→目录单源
//!
//! 七目录闭集（[`DIRS`]：四域+共享+预设+元数据），类→目录查表定位
//! （[`dir_of`] O(1)——闭集定长线性）；资产组织逐件校验+定位（
//! [`AssetRegistry::register`] 每件 O(1) 合计 O(资产数)——锚点「组织
//! O(资产数)」），重名驳回（同库同名是索引歧义的根）。
//!
//! ## 深化三：元数据五元组+引用校验——缺失断言拦截不静默
//!
//! 每特效元数据五元组（[`FxMeta`]）：名（过命名闸）/分类（四域闭集或
//! shared）/语义声明（非空——F5202 语义律同口径，无语义不入册）/自身
//! 预算上限（>0——预算是承诺不是装饰）/参数引用（F5207 schema 名集，
//! 每 ref 非空——引用可解析）。四形缺失断言拦截 [`E_LB_META`] 计数
//! O(1)（锚点「拦截 O(1)」）；全集对拍（[`MetaBook::audit_against`]）
//! 资产清单逐名查册，缺失名册点名——缺谁的说明书一目了然。
//!
//! ## 深化四：迁移清单是步序链——断链倒退拒、apply 不跳步、对拍 O(1)
//!
//! 结构版本从 1 单调起步；登记迁移步 from 必须等于链尾 to（断链
//! [`E_LB_BAD_MIG`]）、to 必须严格大于 from（倒退 [`E_LB_BAD_MIG`]——
//! 结构演进没有时光机）；apply 按链序逐步步进（跳步=漂移计数）；对拍
//! 当前版本与清单终点 O(1)（锚点「对拍 O(1)」）——没走完的迁移就是
//! 漂移，漂移显性化（[`E_LB_DRIFT`]）不静默。

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 错误契约：Z 域字符串码惯例（E_LB_* 前缀）
// ---------------------------------------------------------------------------

/// 命名违规：驳回整改（附专属提示）。
pub const E_LB_NAME: &str = "E_LB_NAME";
/// 元数据缺失：断言拦截（分类/语义/预算/引用任一形）。
pub const E_LB_META: &str = "E_LB_META";
/// 结构漂移：当前版本与迁移清单终点不齐。
pub const E_LB_DRIFT: &str = "E_LB_DRIFT";
/// 迁移清单非法：步序断裂或版本倒退。
pub const E_LB_BAD_MIG: &str = "E_LB_BAD_MIG";

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 4;

/// 错误码→原因串。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_LB_NAME => "命名违规：驳回整改（附专属提示）",
        E_LB_META => "元数据缺失：断言拦截",
        E_LB_DRIFT => "结构漂移：当前版本与迁移终点不齐",
        E_LB_BAD_MIG => "迁移清单非法：步序断裂或版本倒退",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_LB_NAME => "按整改提示改名后重新登记",
        E_LB_META => "补齐缺失元数据字段再入册",
        E_LB_DRIFT => "执行待 apply 的迁移步至终点版本",
        E_LB_BAD_MIG => "修正迁移步序使其接链尾且递增",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 一、命名规范：五规则闭集 + 专属整改提示
// ---------------------------------------------------------------------------

/// 类前缀闭集：fx_p_战斗 / fx_e_环境 / fx_t_演出 / fx_i_界面 / fx_s_共享
/// （F5201 四子系统 + F5202 四域同源口径——前缀即域归属，看名知类）。
pub const NAME_PREFIXES: [&str; 5] = ["fx_p_", "fx_e_", "fx_t_", "fx_i_", "fx_s_"];

/// 命名长度界：下 4（过短无信息量）上 32（过长读不完）。
pub const NAME_LEN_MIN: usize = 4;
pub const NAME_LEN_MAX: usize = 32;

/// **命名规范校验**：五规则闭集逐名过闸（O(1)——定长规则）。
/// 合法 Ok；违规 [`E_LB_NAME`]（提示在 [`hint_of`] 按形给出）。
pub fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.len() < NAME_LEN_MIN || name.len() > NAME_LEN_MAX {
        return Err(E_LB_NAME);
    }
    let b = name.as_bytes();
    if b[0].is_ascii_digit() {
        return Err(E_LB_NAME);
    }
    for &c in b {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_') {
            return Err(E_LB_NAME);
        }
    }
    if name.contains("__") || name.ends_with('_') {
        return Err(E_LB_NAME);
    }
    if classify_of(name).is_none() {
        return Err(E_LB_NAME);
    }
    Ok(())
}

/// **整改提示**（驳回整改——给人指路不是闭门羹）：按违规形给专属
/// 提示；合法名返回「符合规范」（判据侧对拍面）。
pub fn hint_of(name: &str) -> &'static str {
    if name.len() < NAME_LEN_MIN {
        return "过短：至少 4 字符（如 fx_p_rain）";
    }
    if name.len() > NAME_LEN_MAX {
        return "过长：至多 32 字符（截到核心语义词）";
    }
    let b = name.as_bytes();
    if b[0].is_ascii_digit() {
        return "不以数字开头：改用类前缀起头（fx_p_/fx_e_/fx_t_/fx_i_/fx_s_）";
    }
    for &c in b {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_') {
            return "含非法字符：只许小写字母/数字/下划线（小写蛇形）";
        }
    }
    if name.contains("__") {
        return "禁双下划线：单词间单下划线分隔（如 fx_p_hit_spark）";
    }
    if name.ends_with('_') {
        return "禁尾下划线：以下划线结尾是残缺形，补全尾词";
    }
    if classify_of(name).is_none() {
        return "缺类前缀：fx_p_战斗/fx_e_环境/fx_t_演出/fx_i_界面/fx_s_共享";
    }
    "符合规范"
}

// ---------------------------------------------------------------------------
// 二、目录结构规范：七目录闭集 + 前缀→类→目录单源
// ---------------------------------------------------------------------------

/// 四域分类闭集（F5202 同口径）。
pub const FX_DOMAINS: [&str; 4] = ["combat", "ambience", "narrative", "ui"];

/// 七目录闭集：四域+共享+预设+元数据（结构自洽——每类特效有唯一目录）。
pub const DIRS: [(&str, &str); 7] = [
    ("combat", "fx/combat"),
    ("ambience", "fx/ambience"),
    ("narrative", "fx/narrative"),
    ("ui", "fx/ui"),
    ("shared", "fx/shared"),
    ("presets", "fx/presets"),
    ("meta", "fx/meta"),
];

/// 前缀→类：看名知类（O(1)——五前缀定长线性）。
pub fn classify_of(name: &str) -> Option<&'static str> {
    if name.starts_with("fx_p_") {
        Some("combat")
    } else if name.starts_with("fx_e_") {
        Some("ambience")
    } else if name.starts_with("fx_t_") {
        Some("narrative")
    } else if name.starts_with("fx_i_") {
        Some("ui")
    } else if name.starts_with("fx_s_") {
        Some("shared")
    } else {
        None
    }
}

/// 类→目录：查表定位（O(1)——七项闭集线性）。
pub fn dir_of(class: &str) -> Option<&'static str> {
    DIRS.iter().find(|(c, _)| *c == class).map(|(_, d)| *d)
}

/// 单资产条目：名/类/目录三元组（目录是 &'static——闭集成员不拥有）。
#[derive(Clone, Debug)]
pub struct AssetEntry {
    /// 资产名（已过命名闸）。
    pub name: String,
    /// 所属类（前缀映射产出）。
    pub class: String,
    /// 归属目录（闭集快照）。
    pub dir: &'static str,
}

/// 资产登记册：逐件校验+定位（组织 O(资产数)）。
#[derive(Clone, Debug, Default)]
pub struct AssetRegistry {
    assets: Vec<AssetEntry>,
    /// 命名驳回计数（O(1) 告警器）。
    pub name_rejects: u32,
}

impl AssetRegistry {
    /// 空册。
    pub fn new() -> Self {
        AssetRegistry { assets: Vec::new(), name_rejects: 0 }
    }

    /// **资产组织**：命名闸→前缀定类→目录定位→重名查（每件 O(1)
    /// 合计 O(资产数)）。违规驳回 [`E_LB_NAME`] 且**必附整改提示**
    /// （`hint_of`——驳回整改）；Ok 返回归属目录。
    pub fn register(&mut self, name: &str) -> Result<&'static str, &'static str> {
        if validate_name(name).is_err() {
            self.name_rejects += 1;
            return Err(E_LB_NAME);
        }
        let class = match classify_of(name) {
            Some(c) => c,
            None => {
                self.name_rejects += 1;
                return Err(E_LB_NAME);
            }
        };
        let dir = match dir_of(class) {
            Some(d) => d,
            None => {
                self.name_rejects += 1;
                return Err(E_LB_NAME);
            }
        };
        if self.assets.iter().any(|a| a.name == name) {
            self.name_rejects += 1;
            return Err(E_LB_NAME);
        }
        self.assets.push(AssetEntry {
            name: String::from(name),
            class: String::from(class),
            dir,
        });
        Ok(dir)
    }

    /// 资产数。
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// 空册判据。
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// 某名是否在册。
    pub fn has(&self, name: &str) -> bool {
        self.assets.iter().any(|a| a.name == name)
    }

    /// 资产名册快照。
    pub fn names(&self) -> Vec<String> {
        self.assets.iter().map(|a| a.name.clone()).collect()
    }
}

// ---------------------------------------------------------------------------
// 三、元数据规范册：五元组 + 索引 + 全集对拍
// ---------------------------------------------------------------------------

/// 单特效元数据五元组（元数据即索引——没有说明书的产品不上架）。
#[derive(Clone, Debug)]
pub struct FxMeta {
    /// 特效名（已过命名闸）。
    pub name: String,
    /// 分类（四域闭集或 shared）。
    pub class: String,
    /// 语义声明（非空——F5202 语义律同口径：这个特效表达什么内容）。
    pub semantic: String,
    /// 自身预算上限（微秒，>0——预算是承诺不是装饰）。
    pub budget_us: u32,
    /// 参数引用（F5207 schema 名集，每 ref 非空——引用可解析）。
    pub param_refs: Vec<String>,
}

/// 元数据规范册：BTreeMap 索引（查元数据 O(log n)）+ 拦截计数。
#[derive(Clone, Debug, Default)]
pub struct MetaBook {
    book: BTreeMap<String, FxMeta>,
    /// 元数据缺失拦截计数（O(1) 告警器——锚点「拦截 O(1)」）。
    pub missing_blocks: u32,
}

impl MetaBook {
    /// 空册。
    pub fn new() -> Self {
        MetaBook { book: BTreeMap::new(), missing_blocks: 0 }
    }

    /// **登记元数据**：五形缺失断言拦截 [`E_LB_META`]（名违规/分类非
    /// 法/语义空/预算零/引用空串），全过才入索引（O(log n) 插入）。
    pub fn define(
        &mut self,
        name: &str,
        class: &str,
        semantic: &str,
        budget_us: u32,
        param_refs: &[&str],
    ) -> Result<(), &'static str> {
        if validate_name(name).is_err() {
            self.missing_blocks += 1;
            return Err(E_LB_META);
        }
        if class != "shared" && !FX_DOMAINS.contains(&class) {
            self.missing_blocks += 1;
            return Err(E_LB_META);
        }
        if semantic.trim().is_empty() {
            self.missing_blocks += 1;
            return Err(E_LB_META);
        }
        if budget_us == 0 {
            self.missing_blocks += 1;
            return Err(E_LB_META);
        }
        for r in param_refs {
            if r.trim().is_empty() {
                self.missing_blocks += 1;
                return Err(E_LB_META);
            }
        }
        self.book.insert(
            String::from(name),
            FxMeta {
                name: String::from(name),
                class: String::from(class),
                semantic: String::from(semantic),
                budget_us,
                param_refs: param_refs.iter().map(|r| String::from(*r)).collect(),
            },
        );
        Ok(())
    }

    /// **索引查询**（元数据即索引——O(log n)）。
    pub fn lookup(&self, name: &str) -> Option<&FxMeta> {
        self.book.get(name)
    }

    /// 册内元数据条数。
    pub fn len(&self) -> usize {
        self.book.len()
    }

    /// 空册判据。
    pub fn is_empty(&self) -> bool {
        self.book.is_empty()
    }

    /// **全集对拍**：资产清单逐名查册（组织 O(资产数)），缺失名册点名
    /// 返回——缺谁的说明书一目了然（缺失即结构欠账显性化）。
    pub fn audit_against(&self, reg: &AssetRegistry) -> Vec<String> {
        reg.assets
            .iter()
            .filter(|a| !self.book.contains_key(&a.name))
            .map(|a| a.name.clone())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 四、迁移清单：步序链 + 逐步 apply + 漂移对拍
// ---------------------------------------------------------------------------

/// 结构版本起步号（库结构从 1 起步单调演进）。
pub const STRUCT_VERSION_INIT: u32 = 1;

/// 单迁移步：from → to + 演进说明（结构演进走迁移清单——演进可追溯）。
#[derive(Clone, Debug)]
pub struct MigStep {
    /// 起始结构版本。
    pub from_v: u32,
    /// 目标结构版本（严格递增）。
    pub to_v: u32,
    /// 演进说明（非空——演进理由写给人看）。
    pub note: String,
}

/// 迁移清单：步序链 + 当前版本 + 漂移计数。
#[derive(Clone, Debug)]
pub struct MigrationLedger {
    /// 当前结构版本。
    pub current_v: u32,
    /// 清单终点（最后登记步的 to_v——apply 消费链后对拍仍可用的锚）。
    pub target_v: u32,
    steps: Vec<MigStep>,
    /// 漂移/跳步拦截计数（O(1) 告警器）。
    pub drift_blocks: u32,
}

impl MigrationLedger {
    /// 以初版建清单。
    pub fn new() -> Self {
        MigrationLedger {
            current_v: STRUCT_VERSION_INIT,
            target_v: STRUCT_VERSION_INIT,
            steps: Vec::new(),
            drift_blocks: 0,
        }
    }

    /// **登记迁移步**：from 必须接链尾（断链 [`E_LB_BAD_MIG`]）、to 必
    /// 须严格大于 from（倒退 [`E_LB_BAD_MIG`]）、说明非空——三闸全过
    /// 才入链（终点锚随 to 更新——对拍不依赖待消费的链）。
    pub fn plan(&mut self, from_v: u32, to_v: u32, note: &str) -> Result<(), &'static str> {
        let expect = self.steps.last().map(|s| s.to_v).unwrap_or(STRUCT_VERSION_INIT);
        if from_v != expect {
            return Err(E_LB_BAD_MIG);
        }
        if to_v <= from_v {
            return Err(E_LB_BAD_MIG);
        }
        if note.trim().is_empty() {
            return Err(E_LB_BAD_MIG);
        }
        self.steps.push(MigStep {
            from_v,
            to_v,
            note: String::from(note),
        });
        self.target_v = to_v;
        Ok(())
    }

    /// **逐步 apply**：链头步的 from 必须等于当前版本（不跳步——跳步
    /// 即漂移计数），步进后版本更新。返回新版本；无可执行步/跳步返回
    /// None。
    pub fn apply_next(&mut self) -> Option<u32> {
        let next_from = self.steps.first().map(|s| s.from_v)?;
        if next_from != self.current_v {
            self.drift_blocks += 1;
            return None;
        }
        let step = self.steps.remove(0);
        self.current_v = step.to_v;
        Some(step.to_v)
    }

    /// 待 apply 步数。
    pub fn pending(&self) -> usize {
        self.steps.len()
    }

    /// **结构漂移对拍**：当前版本与清单终点对齐 O(1)（锚点「对拍
    /// O(1)」——两次比较）。没走完的迁移就是漂移 → [`E_LB_DRIFT`]。
    /// 终点锚是登记时固化的 target_v（apply 消费链后对拍仍成立——
    /// 「走完了」与「没计划」都回到版本一致态）。
    pub fn audit_structure(&mut self) -> Result<(), &'static str> {
        if self.current_v != self.target_v {
            self.drift_blocks += 1;
            return Err(E_LB_DRIFT);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5209 域自检（判据 12 条六组：目录规范/驳回整改/元数据索引/缺失拦截/版本迁移/判据元）。
pub fn run_vez09_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5209");

    // --- 目录规范（判据一）---
    // 目-01：命名五规则对拍——合法名过闸 + 七形违规全驳。
    let legal = [
        "fx_p_rain_burst",
        "fx_e_fog_glow",
        "fx_t_fade_black",
        "fx_i_panel_pulse",
        "fx_s_common_seed",
    ];
    let mut legal_ok = true;
    for n in legal.iter() {
        if validate_name(n).is_err() || hint_of(n) != "符合规范" {
            legal_ok = false;
        }
    }
    let illegal = [
        "fx",
        "fx_p_rain_burst_that_is_way_too_long_for_a_name_above_limit",
        "1fx_p_rain",
        "fx_p_Rain",
        "fx_p__rain",
        "fx_p_rain_",
        "rain_burst",
    ];
    let mut illegal_ok = illegal.len() == 7;
    for n in illegal.iter() {
        if validate_name(n).is_ok() || hint_of(n) == "符合规范" {
            illegal_ok = false;
        }
    }
    if legal_ok && illegal_ok {
        s.ok("EZ9-目-01-命名五规则七形对拍");
    } else {
        s.fail("EZ9-目-01-命名五规则七形对拍", "命名闸漏");
    }
    // 目-02：目录闭集定位——五类前缀各归其目录 + 目录表自洽（类唯一）。
    let d1 = dir_of("combat") == Some("fx/combat");
    let d2 = dir_of("ambience") == Some("fx/ambience");
    let d3 = dir_of("narrative") == Some("fx/narrative");
    let d4 = dir_of("ui") == Some("fx/ui");
    let d5 = dir_of("shared") == Some("fx/shared");
    let d6 = dir_of("presets") == Some("fx/presets");
    let d7 = dir_of("meta") == Some("fx/meta");
    let mut dirs_unique = true;
    let mut i = 0usize;
    while i < DIRS.len() && dirs_unique {
        let mut j = i + 1;
        while j < DIRS.len() {
            if DIRS[i].0 == DIRS[j].0 || DIRS[i].1 == DIRS[j].1 {
                dirs_unique = false;
            }
            j += 1;
        }
        i += 1;
    }
    if d1 && d2 && d3 && d4 && d5 && d6 && d7 && dirs_unique && DIRS.len() == 7 {
        s.ok("EZ9-目-02-七目录闭集定位自洽");
    } else {
        s.fail("EZ9-目-02-七目录闭集定位自洽", "目录映射漂移");
    }
    // 目-03：资产组织 O(资产数)——三资产各归目录 + 重名驳回 + 计数。
    let mut reg = AssetRegistry::new();
    let r1 = reg.register("fx_p_rain_burst");
    let r2 = reg.register("fx_e_fog_glow");
    let r3 = reg.register("fx_s_common_seed");
    let org_ok = r1 == Ok("fx/combat")
        && r2 == Ok("fx/ambience")
        && r3 == Ok("fx/shared")
        && reg.len() == 3
        && reg.has("fx_p_rain_burst");
    let dup = reg.register("fx_p_rain_burst") == Err(E_LB_NAME)
        && reg.len() == 3
        && reg.name_rejects == 1;
    if org_ok && dup {
        s.ok("EZ9-目-03-资产组织与重名驳回");
    } else {
        s.fail("EZ9-目-03-资产组织与重名驳回", "组织面失效");
    }

    // --- 驳回整改（判据二）---
    // 驳-01：违规注册被驳 + 整改提示与违规形对应（提示可执行非模板句）。
    let mut reg2 = AssetRegistry::new();
    let bad_reg = reg2.register("rain_burst") == Err(E_LB_NAME)
        && reg2.name_rejects == 1
        && hint_of("rain_burst").contains("前缀");
    let bad_len = hint_of("fx").contains("过短");
    let bad_case = hint_of("fx_p_Rain").contains("非法字符");
    let bad_gap = hint_of("fx_p__rain").contains("双下划线");
    let bad_tail = hint_of("fx_p_rain_").contains("尾下划线");
    let bad_head = hint_of("1fx_p_rain").contains("数字开头");
    let bad_long = hint_of("fx_p_rain_burst_that_is_way_too_long_for_a_name_above_limit").contains("过长");
    if bad_reg && bad_len && bad_case && bad_gap && bad_tail && bad_head && bad_long {
        s.ok("EZ9-驳-01-驳回带专属整改提示");
    } else {
        s.fail("EZ9-驳-01-驳回带专属整改提示", "提示面失效");
    }

    // --- 元数据即索引（判据三）---
    // 元-01：五元组登记 + 索引查询字段对拍 + F5207 参数引用可解析。
    let mut book = MetaBook::new();
    let d = book.define("fx_p_rain_burst", "combat", "战斗打击火花——命中反馈", 30, &["intensity", "radius"]);
    let found = book.lookup("fx_p_rain_burst");
    let meta_ok = match (d, found) {
        (Ok(()), Some(m)) => {
            m.class == "combat" && m.budget_us == 30 && m.param_refs.len() == 2
                && m.param_refs[0] == "intensity"
                && !m.semantic.is_empty()
        }
        _ => false,
    };
    // 引用可解析：同名 schema 在 F5207 SchemaBook 登记后可查（消费接缝）。
    let mut sb = crate::svstar2::vez07_params::SchemaBook::new();
    let sd = sb.define("intensity", 0, 100, 40, "特效强度");
    let ref_ok = sd.is_ok() && sb.lookup("intensity").is_some();
    if meta_ok && ref_ok {
        s.ok("EZ9-元-01-五元组索引与引用可解析");
    } else {
        s.fail("EZ9-元-01-五元组索引与引用可解析", "索引面失效");
    }
    // 元-02：五形缺失断言拦截（名违规/分类非法/语义空/预算零/引用空）+ 计数。
    let m1 = book.define("bad name!", "combat", "语义", 30, &[]) == Err(E_LB_META);
    let m2 = book.define("fx_p_good_name", "ghost", "语义", 30, &[]) == Err(E_LB_META);
    let m3 = book.define("fx_p_good_name", "combat", "   ", 30, &[]) == Err(E_LB_META);
    let m4 = book.define("fx_p_good_name", "combat", "语义", 0, &[]) == Err(E_LB_META);
    let m5 = book.define("fx_p_good_name", "combat", "语义", 30, &["intensity", " "]) == Err(E_LB_META);
    let cnt_ok = book.missing_blocks == 5 && book.len() == 1;
    if m1 && m2 && m3 && m4 && m5 && cnt_ok {
        s.ok("EZ9-元-02-五形缺失断言拦截");
    } else {
        s.fail("EZ9-元-02-五形缺失断言拦截", "拦截闸漏");
    }
    // 元-03：全集对拍——3 资产 1 有元数据，缺失名册恰好点名缺的 2 个。
    let mut reg3 = AssetRegistry::new();
    let _ = reg3.register("fx_p_rain_burst");
    let _ = reg3.register("fx_e_fog_glow");
    let _ = reg3.register("fx_t_fade_black");
    let missing = book.audit_against(&reg3);
    let audit_ok = missing.len() == 2
        && missing.iter().any(|n| n == "fx_e_fog_glow")
        && missing.iter().any(|n| n == "fx_t_fade_black")
        && !missing.iter().any(|n| n == "fx_p_rain_burst");
    if audit_ok {
        s.ok("EZ9-元-03-全集对拍缺失点名");
    } else {
        s.fail("EZ9-元-03-全集对拍缺失点名", "对拍失效");
    }

    // --- 版本迁移（判据四）---
    // 迁-01：合法步序链 1→2→3 登记 + 逐步 apply 不跳步 + 终点对齐。
    let mut mig = MigrationLedger::new();
    let p1 = mig.plan(1, 2, "加预设目录");
    let p2 = mig.plan(2, 3, "元数据索引化");
    let a1 = mig.apply_next();
    let a2 = mig.apply_next();
    let chain_ok = p1.is_ok() && p2.is_ok() && a1 == Some(2) && a2 == Some(3)
        && mig.current_v == 3
        && mig.pending() == 0
        && mig.audit_structure().is_ok();
    if chain_ok {
        s.ok("EZ9-迁-01-步序链apply终点对齐");
    } else {
        s.fail("EZ9-迁-01-步序链apply终点对齐", "迁移链失效");
    }
    // 迁-02：断链拒（from 不接链尾）+ 倒退拒（to <= from）+ 空说明拒。
    let mut mig2 = MigrationLedger::new();
    let g1 = mig2.plan(1, 2, "步一");
    let g2 = mig2.plan(1, 3, "断裂——链尾已是 2");
    let g3 = mig2.plan(2, 2, "原地踏步也是倒退");
    let g4 = mig2.plan(2, 1, "倒退");
    let g5 = mig2.plan(2, 3, "  ");
    let bad_mig_ok = g1.is_ok()
        && g2 == Err(E_LB_BAD_MIG)
        && g3 == Err(E_LB_BAD_MIG)
        && g4 == Err(E_LB_BAD_MIG)
        && g5 == Err(E_LB_BAD_MIG)
        && mig2.pending() == 1;
    if bad_mig_ok {
        s.ok("EZ9-迁-02-断链倒退空说明拒");
    } else {
        s.fail("EZ9-迁-02-断链倒退空说明拒", "清单闸漏");
    }
    // 迁-03：结构漂移对拍——未走完即漂移（对拍 O(1)）+ 走完即过。
    let mut mig3 = MigrationLedger::new();
    let _ = mig3.plan(1, 2, "步一");
    let drift = mig3.audit_structure() == Err(E_LB_DRIFT) && mig3.drift_blocks == 1;
    let _ = mig3.apply_next();
    let aligned = mig3.audit_structure().is_ok() && mig3.current_v == 2;
    // 跳步拦：库结构版本被绕过迁移直接改（漂移库），apply 拒不跳步。
    let mut mig5 = MigrationLedger::new();
    let _ = mig5.plan(1, 2, "步一");
    mig5.current_v = 5;
    let skip = mig5.apply_next().is_none() && mig5.drift_blocks == 1 && mig5.current_v == 5;
    if drift && aligned && skip {
        s.ok("EZ9-迁-03-漂移对拍与跳步拦截");
    } else {
        s.fail("EZ9-迁-03-漂移对拍与跳步拦截", "对拍或跳步闸失效");
    }

    // --- 判据元（判据五/六）---
    // 元判据-01：四码互异 + 三要素齐备。
    let codes = [E_LB_NAME, E_LB_META, E_LB_DRIFT, E_LB_BAD_MIG];
    let mut codes_ok = codes.len() == ERROR_COUNT;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if reason_of(codes[i]) == "未知错误码" || direction_of(codes[i]).is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if codes_ok {
        s.ok("EZ9-判据-01-码互异三要素齐备");
    } else {
        s.fail("EZ9-判据-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元判据-02：判据面零 panic 零 unsafe（自扫本文件，单遍词法剥除）。
    let src = include_str!("vez09_libstruct.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EZ9-判据-02-零panic零unsafe面");
    } else {
        s.fail("EZ9-判据-02-零panic零unsafe面", "判据/生产面含 panic 或 unsafe");
    }
    // 元判据-03：域口径自洽——五前缀与四域+shared 一一对应可互推。
    let mut self_ok = NAME_PREFIXES.len() == 5 && FX_DOMAINS.len() == 4;
    for p in NAME_PREFIXES.iter() {
        let mut probe = String::from(*p);
        probe.push_str("probe");
        match classify_of(&probe) {
            Some("shared") => {
                if *p != "fx_s_" {
                    self_ok = false;
                }
            }
            Some(dom) => {
                if !FX_DOMAINS.contains(&dom) || *p == "fx_s_" {
                    self_ok = false;
                }
                if dir_of(dom).is_none() {
                    self_ok = false;
                }
            }
            None => self_ok = false,
        }
    }
    if self_ok {
        s.ok("EZ9-判据-03-前缀域目录一一对应");
    } else {
        s.fail("EZ9-判据-03-前缀域目录一一对应", "口径不自洽");
    }

    s
}

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
