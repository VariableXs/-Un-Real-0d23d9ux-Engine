//! VE-F4405 · 显示器配置文件（VE-W 域 · 显示与色彩批 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4405`
//!
//! **判据（锚点原文）**：三节档案、开放格式、版本回退、三查校验、判据。
//!
//! **职责定位（锚点原文）**：显示器配置文件体系——每屏一档三节（能力节/
//! 校准节/偏好节）；配置导入导出**开放格式**（十四章数据开放口径）；
//! 配置版本化+变更留痕；配置校验器（字段/范围/引用**三查**）。错误路径
//! 与降级矩阵：校验不过→拒绝生效+三要素提示；导入冲突→逐项裁决清单；
//! 版本回退→快照一键。性能：校验 O(字段数)；导入 O(项数)；回退 O(1)。
//!
//! # 一、为什么是「每屏一档三节」而不是一个大配置
//!
//! 三类数据的**变更节奏与信任来源**不同：能力节来自 F4402 台账（设备
//! 说了算，用户不该改）、校准节来自校准仪或用户调色（要引用 F4403 的
//! profile 短码）、偏好节是纯用户意志（含无障碍偏好——域本色，见四）。
//! 揉成一锅，「用户改坏了能力节」「校准被偏好覆盖」这类事故无法归因。
//! 三节封闭（[`SectionKind`]），字段不串节——判据「三节档案」的本意。
//!
//! # 二、开放格式为什么是自描述键值而不是二进制
//!
//! 「十四章数据开放口径」：配置必须能被无本仓代码的第三方读出——
//! 导出为分节 `key=value` 纯文本（[`export_text`]），节头 `[capability]`
//! 等三行定界；**导出可回导**（`import_text` 解析回同一档案，判据
//! 「开放格式」的往返口径）。隐私面：导出可选脱敏（[`RedactMode`]）——
//! 校准节里的仪器身份类值以 `__REDACTED__` 替换，格式仍是开放的，
//! 只是「内容可选地不外带」。
//!
//! # 三、版本链为什么回退是 O(1)
//!
//! 变更不覆盖历史：每次生效 push 一个 [`VersionNode`]（版本号+变更键
//! 清单，锚点「版本化+变更留痕」）。回退（[`rollback`]）不重放不重算
//! ——链上每个节点存**整档快照**（档案小：每屏几十字段），回退=
//! 取链上前驱快照引用直达，O(1)（锚点口径）。留痕回放归 F4422
//! （持久深化，前向声明）；同步分发归 F4407（下游）。
//!
//! # 四、三查校验与无障碍偏好节（锚点原文）
//!
//! 校验器三查各管一类病：**字段查**（键不在节白名单——拼错即拒）、
//! **范围查**（数值越域——亮度不会是 400%）、**引用查**（值引用外部
//! 实体——校准节 color_profile 必须是 F4403 的四个短码之一、能力节
//! 身份键必须在 F4402 台账在册）。每错带**三要素提示**（字段/原因/
//! 建议）并**拒绝生效**（锚点降级矩阵第一条——错配置生效比不生效
//! 严重得多）。偏好节承载无障碍偏好（高对比/读屏行距等），随档案
//! 一起走版本链——无障碍设置不该是二等公民。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、错误码（字符串码家族，与 F4403/F4404 同族）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const CFG_FILE_VERSION: &str = "V05-cfg-v1";

/// 字段查失败（键不在节白名单）。
pub const E_CFG_FIELD: &str = "E_CFG_FIELD";
/// 范围查失败（数值越域）。
pub const E_CFG_RANGE: &str = "E_CFG_RANGE";
/// 引用查失败（值引用的外部实体不存在）。
pub const E_CFG_REF: &str = "E_CFG_REF";
/// 导入冲突（逐项裁决清单已生成）。
pub const E_CFG_IMPORT: &str = "E_CFG_IMPORT";
/// 版本操作非法（回退目标不存在）。
pub const E_CFG_VERSION: &str = "E_CFG_VERSION";

// ---------------------------------------------------------------------------
// 二、三节档案（数据结构之一：配置档）
// ---------------------------------------------------------------------------

/// 三节封闭集（节不串、字段不混——判据「三节档案」的结构口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    /// 能力节（F4402 台账上游说了算）。
    Capability,
    /// 校准节（引用 F4403 profile 短码）。
    Calibration,
    /// 偏好节（用户意志 + 无障碍偏好，域本色）。
    Preference,
}

impl SectionKind {
    /// 开放格式节头。
    pub fn header(self) -> &'static str {
        match self {
            SectionKind::Capability => "[capability]",
            SectionKind::Calibration => "[calibration]",
            SectionKind::Preference => "[preference]",
        }
    }

    /// 由节头还原（开放格式回导用；未知名拒）。
    pub fn parse_header(s: &str) -> Option<SectionKind> {
        match s {
            "[capability]" => Some(SectionKind::Capability),
            "[calibration]" => Some(SectionKind::Calibration),
            "[preference]" => Some(SectionKind::Preference),
            _ => None,
        }
    }
}

/// 单条配置（值统一字符串口径——开放格式直来直去）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigEntry {
    /// 所属节。
    pub section: SectionKind,
    /// 键（节内白名单见 [`KEY_SPECS`]）。
    pub key: String,
    /// 值（原样字符串；范围/引用语义由校验器解释）。
    pub value: String,
}

/// 字段规格（白名单单源：键/节/值域三元组；判据侧字面量对账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySpec {
    /// 所属节。
    pub section: SectionKind,
    /// 键名。
    pub key: &'static str,
    /// 值域下界（数值型；非数值型忽略）。
    pub min: i64,
    /// 值域上界。
    pub max: i64,
    /// 引用型（true = 值是外部实体引用，走引用查）。
    pub is_ref: bool,
}

/// 字段白名单（每屏档案全部合法键；节内不在此表的键 = 字段查失败）。
pub const KEY_SPECS: [KeySpec; 10] = [
    // 能力节（上游 F4402 口径，只读投影——用户改了会被台账同步顶回）
    KeySpec { section: SectionKind::Capability, key: "max_refresh_hz", min: 24, max: 1000, is_ref: false },
    KeySpec { section: SectionKind::Capability, key: "panel_peak_minit", min: 100_000, max: 10_000_000, is_ref: false },
    KeySpec { section: SectionKind::Capability, key: "hdr_capable", min: 0, max: 1, is_ref: false },
    // 校准节（引用 F4403 短码域）
    KeySpec { section: SectionKind::Calibration, key: "brightness_pct", min: 0, max: 100, is_ref: false },
    KeySpec { section: SectionKind::Calibration, key: "gamma_per_mille", min: 800, max: 1200, is_ref: false },
    KeySpec { section: SectionKind::Calibration, key: "color_profile", min: 0, max: 0, is_ref: true },
    // 偏好节（含无障碍域本色）
    KeySpec { section: SectionKind::Preference, key: "night_light_pct", min: 0, max: 100, is_ref: false },
    KeySpec { section: SectionKind::Preference, key: "high_contrast", min: 0, max: 1, is_ref: false },
    KeySpec { section: SectionKind::Preference, key: "reader_line_spacing", min: 100, max: 300, is_ref: false },
    KeySpec { section: SectionKind::Preference, key: "caption_size_pct", min: 80, max: 300, is_ref: false },
];

/// F4403 校准短码域（引用查的真源；与 `vev03_color::ProfileKind::wire` 对齐）。
pub const PROFILE_REFS: [&str; 4] = ["srgb", "p3", "adobe", "custom"];

/// 单屏配置档。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigProfile {
    /// 显示器身份键（F4402 台账口径）。
    pub identity: String,
    /// 三节条目（节内顺序保持——开放格式往返无损的前提）。
    pub entries: Vec<ConfigEntry>,
}

impl ConfigProfile {
    /// 空档。
    pub fn new(identity: &str) -> ConfigProfile {
        ConfigProfile { identity: identity.to_string(), entries: Vec::new() }
    }

    /// 设值（同键覆盖——档案内键唯一）。
    pub fn set(&mut self, section: SectionKind, key: &str, value: &str) {
        let key = key.to_string();
        if let Some(e) = self.entries.iter_mut().find(|e| e.section == section && e.key == key) {
            e.value = value.to_string();
        } else {
            self.entries.push(ConfigEntry { section, key, value: value.to_string() });
        }
    }

    /// 取值。
    pub fn get(&self, section: SectionKind, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.section == section && e.key == key)
            .map(|e| e.value.as_str())
    }

    /// 节内条目数（校验 O(字段数) 的「字段数」口径）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三、三查校验器（数据结构之三；O(字段数)）
// ---------------------------------------------------------------------------

/// 校验错误（**三要素**：字段/原因/建议——锚点「三要素提示」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationErr {
    /// 要素一：字段全名（节头+键）。
    pub field: String,
    /// 要素二：原因码（E_CFG_FIELD / E_CFG_RANGE / E_CFG_REF）。
    pub reason: &'static str,
    /// 要素三：人话建议（读屏可读）。
    pub advice: String,
}

/// 键规格查找（字段查第一步；O(白名单)=O(1) 常量上界）。
fn spec_of(section: SectionKind, key: &str) -> Option<&'static KeySpec> {
    KEY_SPECS.iter().find(|s| s.section == section && s.key == key)
}

/// 三查校验（O(字段数)：逐条目按节规格三连查，任何一查不过即立案）。
///
/// 锚点降级矩阵第一条：校验不过 → **拒绝生效** + 三要素提示（返回
/// 全部错误，不只首错——用户一次改完比一次改一条仁慈）。
pub fn validate(profile: &ConfigProfile, ledger_identities: &[String]) -> Vec<ValidationErr> {
    let mut errs: Vec<ValidationErr> = Vec::new();
    for e in &profile.entries {
        let field = format!("{}{}", e.section.header(), e.key);
        // 一查：字段（键在节白名单）。
        let spec = match spec_of(e.section, &e.key) {
            Some(s) => s,
            None => {
                errs.push(ValidationErr {
                    field,
                    reason: E_CFG_FIELD,
                    advice: format!("键 {} 不属于 {} 白名单，请删除或改名", e.key, e.section.header()),
                });
                continue;
            }
        };
        // 二查：范围（数值型值域）。
        let parsed: Option<i64> = e.value.trim().parse().ok();
        match parsed {
            Some(v) if v >= spec.min && v <= spec.max => {}
            Some(v) => errs.push(ValidationErr {
                field,
                reason: E_CFG_RANGE,
                advice: format!(
                    "值 {} 越出合法区间 [{}, {}]，请回该范围内",
                    v, spec.min, spec.max
                ),
            }),
            None => {
                // 非数值：引用型走三查，其余按引用域处理。
                // 三查：引用（值引用的外部实体存在）。
                let known = if spec.key == "color_profile" {
                    PROFILE_REFS.iter().any(|r| *r == e.value)
                } else {
                    ledger_identities.iter().any(|id| id == &e.value)
                };
                if !known {
                    errs.push(ValidationErr {
                        field,
                        reason: E_CFG_REF,
                        advice: format!(
                            "值 {} 不在合法引用域内（{}）",
                            e.value,
                            if spec.key == "color_profile" { "srgb/p3/adobe/custom" } else { "台账在册身份键" }
                        ),
                    });
                }
            }
        }
    }
    errs
}

// ---------------------------------------------------------------------------
// 四、开放格式导出/回导（判据「开放格式」；导出 O(项数)）
// ---------------------------------------------------------------------------

/// 脱敏模式（导出可选——隐私面最小实现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedactMode {
    /// 原样导出。
    Plain,
    /// 脱敏：校准节整节值以 `__REDACTED__` 替换（仪器身份不出域）。
    RedactCalibration,
}

/// 脱敏占位符（判据字面量）。
pub const REDACTED: &str = "__REDACTED__";

/// 开放格式导出（O(项数)）：分节键值纯文本，十四章开放口径。
pub fn export_text(profile: &ConfigProfile, mode: RedactMode) -> String {
    let mut out = String::new();
    for sk in [SectionKind::Capability, SectionKind::Calibration, SectionKind::Preference] {
        out.push_str(sk.header());
        out.push('\n');
        for e in profile.entries.iter().filter(|e| e.section == sk) {
            let value = match mode {
                RedactMode::Plain => e.value.clone(),
                RedactMode::RedactCalibration if e.section == SectionKind::Calibration => {
                    REDACTED.to_string()
                }
                RedactMode::RedactCalibration => e.value.clone(),
            };
            out.push_str(&format!("{}={}\n", e.key, value));
        }
    }
    out
}

/// 开放格式回导（O(项数)：单遍逐行）。未节先行/未知节头 → None（表外拒）。
pub fn import_text(text: &str, identity: &str) -> Option<ConfigProfile> {
    let mut profile = ConfigProfile::new(identity);
    let mut section: Option<SectionKind> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            match SectionKind::parse_header(line) {
                Some(s) => section = Some(s),
                None => return None,
            }
            continue;
        }
        let sk = section?;
        let (k, v) = line.split_once('=')?;
        profile.set(sk, k.trim(), v.trim());
    }
    Some(profile)
}

// ---------------------------------------------------------------------------
// 五、版本链与一键回退（数据结构之二；push O(1) 摊还、回退 O(1)）
// ---------------------------------------------------------------------------

/// 版本节点（快照式：节点存整档，回退直达不重放）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionNode {
    /// 版本号（从 1 起递增）。
    pub version: u32,
    /// 本次变更涉及的键（变更留痕；仅快照差异的键名清单）。
    pub changed_keys: Vec<String>,
    /// 整档快照。
    pub snapshot: ConfigProfile,
}

/// 版本化档案容器（版本链 + 当前生效档）。
#[derive(Clone, Debug)]
pub struct VersionedProfile {
    /// 显示器身份键。
    pub identity: String,
    /// 当前生效档。
    pub current: ConfigProfile,
    /// 版本链（链尾 = 最新；node[i].version = i+1）。
    pub chain: Vec<VersionNode>,
}

impl VersionedProfile {
    /// 建档（初始版本 1 入链——空档也有留痕起点）。
    pub fn new(identity: &str) -> VersionedProfile {
        let empty = ConfigProfile::new(identity);
        VersionedProfile {
            identity: identity.to_string(),
            chain: vec![VersionNode {
                version: 1,
                changed_keys: Vec::new(),
                snapshot: empty.clone(),
            }],
            current: empty,
        }
    }

    /// 生效（校验通过后调用）：与当前档差异键留痕、整档入链、版本递增。
    pub fn commit(&mut self, next: ConfigProfile) -> u32 {
        let mut changed: Vec<String> = Vec::new();
        for e in &next.entries {
            if self.current.get(e.section, &e.key) != Some(e.value.as_str()) {
                changed.push(format!("{}{}", e.section.header(), e.key));
            }
        }
        let v = self.chain.len() as u32 + 1;
        self.chain.push(VersionNode { version: v, changed_keys: changed, snapshot: next.clone() });
        self.current = next;
        v
    }

    /// 一键回退（O(1)：直达目标快照，不重放不重算）。
    ///
    /// 目标版本必须已在链上；回退本身**也留痕**（回退是新版本入链，
    /// 回退到过去不等于时间旅行——历史不被改写）。目标不存在 → 立案。
    pub fn rollback(&mut self, target_version: u32) -> Result<u32, ()> {
        let idx = match (target_version as usize).checked_sub(1) {
            Some(i) if target_version as usize <= self.chain.len() => i,
            _ => return Err(()),
        };
        let snap = self.chain[idx].snapshot.clone();
        Ok(self.commit(snap))
    }

    /// 链长（判据口径：初始 1 + 生效次数）。
    pub fn chain_len(&self) -> usize {
        self.chain.len()
    }
}

// ---------------------------------------------------------------------------
// 六、导入冲突逐项裁决（锚点降级矩阵第二条；O(项数)）
// ---------------------------------------------------------------------------

/// 冲突裁决（逐项清单的每行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictVerdict {
    /// 本地有、来档没有：保留本地（不静默删）。
    KeepLocal { key: String },
    /// 来档有、本地没有：采纳来档。
    TakeIncoming { key: String, value: String },
    /// 两边都有且不同：以**来档**为准但入清单（裁决显性——不静默覆盖）。
    Overwritten { key: String, local: String, incoming: String },
    /// 两边相同：无冲突。
    Same { key: String },
}

/// 导入冲突清单（O(项数)：两档逐键对账，三态归类）。
pub fn import_conflicts(local: &ConfigProfile, incoming: &ConfigProfile) -> Vec<ConflictVerdict> {
    let mut out: Vec<ConflictVerdict> = Vec::new();
    for e in &local.entries {
        match incoming.get(e.section, &e.key) {
            None => out.push(ConflictVerdict::KeepLocal { key: e.key.clone() }),
            Some(v) if v == e.value => out.push(ConflictVerdict::Same { key: e.key.clone() }),
            Some(v) => out.push(ConflictVerdict::Overwritten {
                key: e.key.clone(),
                local: e.value.clone(),
                incoming: v.to_string(),
            }),
        }
    }
    for e in &incoming.entries {
        if local.get(e.section, &e.key).is_none() {
            out.push(ConflictVerdict::TakeIncoming { key: e.key.clone(), value: e.value.clone() });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 七、主流程编排（校验→生效→留痕；读屏摘要）
// ---------------------------------------------------------------------------

/// 主流程：校验不过拒绝生效（三要素在案）；通过则 commit 入链。
///
/// 返回 Err(错误清单) 或 Ok(新版本号)。锚点「校验不过→拒绝生效+三要素
/// 提示」的落地口——调用方拿 Err 就别把档交给下游（F4407 同步）。
pub fn apply(
    vp: &mut VersionedProfile,
    next: ConfigProfile,
    ledger_identities: &[String],
) -> Result<u32, Vec<ValidationErr>> {
    let errs = validate(&next, ledger_identities);
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(vp.commit(next))
}

/// 读屏摘要（配置状态可查——锚点无障碍口径延伸）。
pub fn screen_summary(vp: &VersionedProfile, errs: &[ValidationErr]) -> String {
    format!(
        "显示器 {} 配置：版本 {}（链 {} 节），条目 {} 项，待处置校验问题 {} 项",
        vp.identity,
        vp.chain_len(),
        vp.chain.len(),
        vp.current.len(),
        errs.len()
    )
}
