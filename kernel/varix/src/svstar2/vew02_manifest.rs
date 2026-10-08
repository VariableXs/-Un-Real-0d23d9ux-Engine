//! VE-F4602 · 插件清单格式 Manifest（VE-W 域 · 插件 SDK · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4602`
//!
//! **判据（锚点原文四条）**：**四节清单、schema 校验、最小缺省、升级提示**。
//!
//! - **四节清单**：插件声明式清单四节——**身份节**（ID/名称/版本/作者）、
//!   **能力节**（所需权限与 API 面）、**入口节**（激活点）、**兼容节**（宿主版本范围）；
//!   另含**无障碍符合性声明节**（锚点「无障碍与隐私」明列，域本色）。
//! - **schema 校验**：格式版本化 + 校验器，**声明不实者加载期拒绝**——校验不过
//!   不是警告而是拒绝加载 + 三要素提示。
//! - **最小缺省**：**能力声明缺失 → 按最小权限缺省**（不给就是不给，绝不按
//!   「大概需要」补全——最小权限是缺省方向，不是最大便利方向）。
//! - **升级提示**：格式版本不识别 → **明确提示升级**，不静默按旧版解析。
//!
//! **错误路径与降级矩阵**：schema 校验不过 → 拒绝加载 + 三要素提示；能力声明缺失 →
//! 按最小权限缺省；格式版本不识别 → 明确提示升级。
//!
//! **跨批对接点**：上游 F4601 架构（能力声明须落在冻结能力面内）；下游 F4603 加载器
//! 消费、F4642 权限声明衔接。
//!
//! **无障碍与隐私**：清单含无障碍符合性声明节（域本色）；**作者信息非隐私**——本条
//! 不索取也不存储任何个人可识别信息，只留作者署名串。
//!
//! 逻辑 tick 注入，零墙钟；零 IO；类型自持（不 import 未注册兄弟模块——平行会话
//! 的 `ve*` 族尚在施工，编译期硬耦合会让本条因别人的进度而红）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 本条支持的清单格式版本（当前版本）。
pub const MANIFEST_FORMAT_VERSION: u16 = 2;

/// 可识别的最低格式版本（低于此明确提示升级）。
pub const MIN_SUPPORTED_FORMAT_VERSION: u16 = 1;

/// 清单四节 + 无障碍节（节数是契约）。
pub const MANIFEST_SECTION_COUNT: usize = 5;

/// 标识符长度上界（防止超长标识拖慢比较；防御性上界）。
pub const MAX_ID_LEN: usize = 64;

/// 署名串长度上界（作者信息非隐私，但仍需上界防御）。
pub const MAX_AUTHOR_LEN: usize = 64;

/// 能力项上界（防止清单声明海量权限）。
pub const MAX_CAPABILITIES: usize = 64;

/// 入口项上界。
pub const MAX_ENTRY_POINTS: usize = 16;

/// 激活点种类（入口节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationPoint {
    /// 加载即激活。
    OnLoad,
    /// 首次调用某能力时激活。
    OnFirstUse,
    /// 用户显式启用。
    OnUserEnable,
    /// 命中某事件时激活。
    OnEvent,
}

impl ActivationPoint {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            ActivationPoint::OnLoad => "on-load",
            ActivationPoint::OnFirstUse => "on-first-use",
            ActivationPoint::OnUserEnable => "on-user-enable",
            ActivationPoint::OnEvent => "on-event",
        }
    }

    /// 四种穷举（入口节完整性机检的驱动面）。
    pub fn all() -> [ActivationPoint; 4] {
        [
            ActivationPoint::OnLoad,
            ActivationPoint::OnFirstUse,
            ActivationPoint::OnUserEnable,
            ActivationPoint::OnEvent,
        ]
    }
}

/// 能力面（能力节；须落在 F4601 冻结能力面内）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// 显示与色彩 API 面（承接 F4593 交接面之一）。
    DisplayColor,
    /// 无障碍门禁面（承接 F4593 交接面之一）。
    A11yGate,
    /// 一致性契约面（承接 F4593 交接面之一）。
    Consistency,
    /// 文件读写。
    FileReadWrite,
    /// 网络访问。
    Network,
}

impl Capability {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            Capability::DisplayColor => "display-color",
            Capability::A11yGate => "a11y-gate",
            Capability::Consistency => "consistency",
            Capability::FileReadWrite => "file-rw",
            Capability::Network => "network",
        }
    }

    /// 五项穷举。
    pub fn all() -> [Capability; 5] {
        [
            Capability::DisplayColor,
            Capability::A11yGate,
            Capability::Consistency,
            Capability::FileReadWrite,
            Capability::Network,
        ]
    }

    /// **最小权限缺省集**：能力声明缺失时按此缺省。
    ///
    /// 只有纯声明性的零风险能力——不碰文件、不碰网络。**缺省方向是收敛不是便利**：
    /// 少给权限最多让插件功能受限，多给权限则可能越权。
    pub const MINIMAL_DEFAULT: [Capability; 1] = [Capability::Consistency];
}

/// 无障碍符合性（无障碍声明节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A11yConformance {
    /// 已声明符合。
    Declared,
    /// 未声明（缺省不得视为符合——沉默不等于合规）。
    Undeclared,
}

// ---------------------------------------------------------------------------
// 二、数据结构（四节 + 无障碍节）
// ---------------------------------------------------------------------------

/// 身份节。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentitySection {
    /// 稳定 ID（唯一且非空）。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 插件版本。
    pub version: String,
    /// 作者署名（非隐私：仅署名串，不索取个人可识别信息）。
    pub author: String,
}

/// 能力节。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilitySection {
    /// 所需权限（所需即所需，不等于已授予）。
    pub requested: Vec<Capability>,
    /// 是否显式声明了这一节（false 即走最小缺省）。
    pub declared: bool,
}

/// 入口节。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntrySection {
    /// 激活点。
    pub point: ActivationPoint,
    /// 激活点参数（OnEvent 时为事件名；其余为空）。
    pub argument: String,
}

/// 兼容节。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatSection {
    /// 宿主版本下界（含）。
    pub host_min: String,
    /// 宿主版本上界（含）。
    pub host_max: String,
}

/// 插件清单（五节容器；格式版本随清单走）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// 清单格式版本（不识别即提示升级）。
    pub format_version: u16,
    /// 身份节。
    pub identity: IdentitySection,
    /// 能力节。
    pub capability: CapabilitySection,
    /// 入口节。
    pub entry: EntrySection,
    /// 兼容节。
    pub compat: CompatSection,
    /// 无障碍符合性声明节（域本色）。
    pub a11y: A11yConformance,
}

/// 校验诊断（三要素：发生了什么、为什么、下一步）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 错误码。
    pub code: &'static str,
    /// 三要素之一：发生了什么。
    pub what: String,
    /// 三要素之二：为什么。
    pub why: &'static str,
    /// 三要素之三：下一步怎么办。
    pub next: &'static str,
}

// ---------------------------------------------------------------------------
// 三、校验器（四条判据的落点）
// ---------------------------------------------------------------------------

/// 校验结果（加载期裁决）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// 是否准加载。
    pub loadable: bool,
    /// 诊断清单（非空即拒绝；零静默）。
    pub diagnostics: Vec<Diagnostic>,
    /// 生效能力集（校验后；未声明能力节时为最小缺省集）。
    pub effective: Vec<Capability>,
}

impl Verdict {
    /// 首个错误码（便于上层快速分派；无诊断返回空串）。
    pub fn first_code(&self) -> &'static str {
        self.diagnostics.first().map(|d| d.code).unwrap_or("")
    }
}

/// 版本号比较（点分十进制；逐段比，段数不等以较短者补 0）。
fn version_cmp(a: &str, b: &str) -> core::cmp::Ordering {
    let pa: Vec<u32> = a
        .split('.')
        .map(|s| s.parse::<u32>().unwrap_or(0))
        .collect();
    let pb: Vec<u32> = b
        .split('.')
        .map(|s| s.parse::<u32>().unwrap_or(0))
        .collect();
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            core::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    core::cmp::Ordering::Equal
}

/// 校验清单（加载期唯一入口；声明不实者在此被拒）。
///
/// 复杂度 O(字段数)：身份 4 字段 + 能力项 + 入口 + 兼容 + 无障碍声明。
pub fn validate(m: &Manifest) -> Verdict {
    let mut d: Vec<Diagnostic> = Vec::new();

    // ---- 格式版本：不识别即明确提示升级（不静默按旧版解析）----
    if m.format_version < MIN_SUPPORTED_FORMAT_VERSION || m.format_version > MANIFEST_FORMAT_VERSION {
        d.push(Diagnostic {
            code: "E_FORMAT_VERSION_UNSUPPORTED",
            what: format!(
                "清单格式版本 {} 不在支持区间 [{}, {}]",
                m.format_version, MIN_SUPPORTED_FORMAT_VERSION, MANIFEST_FORMAT_VERSION
            ),
            why: "格式版本不识别：按旧版解析会把新字段当未知字段丢掉，语义静默漂移",
            next: "升级插件 SDK 清单格式到受支持版本后重新提交",
        });
    }

    // ---- 身份节：ID/名称非空且在长度界内 ----
    if m.identity.id.is_empty() {
        d.push(Diagnostic {
            code: "E_ID_EMPTY",
            what: "身份节 ID 为空".to_string(),
            why: "ID 是插件的稳定标识，缺失则加载器无法寻址该插件",
            next: "在身份节填写非空 ID",
        });
    } else if m.identity.id.len() > MAX_ID_LEN {
        d.push(Diagnostic {
            code: "E_ID_TOO_LONG",
            what: format!("身份节 ID 长度 {} 超上界 {}", m.identity.id.len(), MAX_ID_LEN),
            why: "超长标识拖慢逐项比较，且 ID 不应承载描述性内容",
            next: "改用短稳定 ID，描述性内容放名称节",
        });
    }
    if m.identity.name.is_empty() {
        d.push(Diagnostic {
            code: "E_NAME_EMPTY",
            what: "身份节名称为空".to_string(),
            why: "名称是加载器与用户界面显示的唯一凭据",
            next: "在身份节填写显示名称",
        });
    }
    if m.identity.version.is_empty() {
        d.push(Diagnostic {
            code: "E_VERSION_EMPTY",
            what: "身份节版本为空".to_string(),
            why: "版本参与兼容与缓存键判定，缺失将无法判定新旧",
            next: "在身份节填写插件版本（如 1.0.0）",
        });
    }
    if m.identity.author.is_empty() {
        d.push(Diagnostic {
            code: "E_AUTHOR_EMPTY",
            what: "身份节作者署名为空".to_string(),
            why: "署名是插件来源的可追溯凭据（非隐私，仅署名串）",
            next: "在身份节填写作者署名",
        });
    } else if m.identity.author.len() > MAX_AUTHOR_LEN {
        d.push(Diagnostic {
            code: "E_AUTHOR_TOO_LONG",
            what: format!(
                "作者署名长度 {} 超上界 {}",
                m.identity.author.len(),
                MAX_AUTHOR_LEN
            ),
            why: "署名只需标识来源，超长无益且可能夹带个人信息",
            next: "改用简短署名；清单不承载个人可识别信息",
        });
    }

    // ---- 能力节：未声明走最小缺省；已声明则查重复与越界 ----
    let effective: Vec<Capability> = if m.capability.declared {
        let mut v = Vec::new();
        for c in m.capability.requested.iter() {
            if v.contains(c) {
                d.push(Diagnostic {
                    code: "E_CAPABILITY_DUPLICATE",
                    what: format!("能力节重复声明 {}", c.tag()),
                    why: "重复项会让「声明即所需」的计数失真，掩盖越权",
                    next: "删除重复的能力声明项",
                });
            }
            v.push(*c);
        }
        if v.len() > MAX_CAPABILITIES {
            d.push(Diagnostic {
                code: "E_CAPABILITY_TOO_MANY",
                what: format!("能力项 {} 超上界 {}", v.len(), MAX_CAPABILITIES),
                why: "海量能力声明等于放弃最小权限自律",
                next: "把能力项压到上界以内并拆分插件",
            });
        }
        v
    } else {
        // 最小缺省：不声明即不给，只给零风险的声明性能力。
        Capability::MINIMAL_DEFAULT.to_vec()
    };

    // ---- 入口节：激活点合法 + OnEvent 须带参数 ----
    if m.entry.point == ActivationPoint::OnEvent && m.entry.argument.is_empty() {
        d.push(Diagnostic {
            code: "E_ENTRY_ARG_MISSING",
            what: "入口节声明 on-event 但未给出事件名".to_string(),
            why: "on-event 无事件名则入口永不触发，等于声明了一个假入口",
            next: "在入口节填写事件名，或改用其他激活点",
        });
    }

    // ---- 兼容节：区间须非空且下界不高于上界 ----
    if m.compat.host_min.is_empty() || m.compat.host_max.is_empty() {
        d.push(Diagnostic {
            code: "E_COMPAT_RANGE_INCOMPLETE",
            what: "兼容节宿主版本区间不完整".to_string(),
            why: "区间缺失则加载器无法判定宿主是否满足兼容声明",
            next: "同时填写宿主版本下界与上界",
        });
    } else if version_cmp(&m.compat.host_min, &m.compat.host_max) == core::cmp::Ordering::Greater
    {
        d.push(Diagnostic {
            code: "E_COMPAT_RANGE_INVERTED",
            what: format!(
                "兼容节区间反转：下界 {} 高于上界 {}",
                m.compat.host_min, m.compat.host_max
            ),
            why: "反转区间恒为空集，任何宿主都判不兼容——属声明不实",
            next: "修正版本区间使下界不高于上界",
        });
    }

    // ---- 无障碍声明节：沉默不等于合规（缺省记未声明，不阻断加载）----
    // 未声明不拒绝加载（无障碍基线由 F4642 授权侧把门），但绝不静默当合规。

    // 生效能力集去重（防御：缺省集与声明集都保证唯一）。
    let mut uniq: Vec<Capability> = Vec::new();
    for c in effective.into_iter() {
        if !uniq.contains(&c) {
            uniq.push(c);
        }
    }

    Verdict {
        loadable: d.is_empty(),
        diagnostics: d,
        effective: uniq,
    }
}

/// 判定宿主是否落在兼容区间内。
pub fn host_supported(m: &Manifest, host_version: &str) -> bool {
    !m.compat.host_min.is_empty()
        && !m.compat.host_max.is_empty()
        && version_cmp(host_version, &m.compat.host_min) != core::cmp::Ordering::Less
        && version_cmp(host_version, &m.compat.host_max) != core::cmp::Ordering::Greater
}

/// 构造一份合规清单（自检与下游共用底座）。
pub fn compliant_manifest() -> Manifest {
    Manifest {
        format_version: MANIFEST_FORMAT_VERSION,
        identity: IdentitySection {
            id: "com.example.wallpaper".to_string(),
            name: "示例壁纸插件".to_string(),
            version: "1.0.0".to_string(),
            author: "example".to_string(),
        },
        capability: CapabilitySection {
            requested: vec![Capability::DisplayColor, Capability::A11yGate],
            declared: true,
        },
        entry: EntrySection {
            point: ActivationPoint::OnLoad,
            argument: String::new(),
        },
        compat: CompatSection {
            host_min: "2.0.0".to_string(),
            host_max: "3.0.0".to_string(),
        },
        a11y: A11yConformance::Declared,
    }
}

// ---------------------------------------------------------------------------
// 四、自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F4602 域自检。
pub fn run_vew02_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F4602");

    // ---- 判据一：四节清单 ----
    {
        let v = validate(&compliant_manifest());
        set.add(
            "W02-四节-合规清单准加载",
            v.loadable && v.diagnostics.is_empty(),
            "",
        );
    }

    {
        // 五节齐备（身份/能力/入口/兼容/无障碍）由结构体字段承载，机检其覆盖。
        let m = compliant_manifest();
        let ok = !m.identity.id.is_empty()
            && m.capability.declared
            && m.entry.point == ActivationPoint::OnLoad
            && !m.compat.host_min.is_empty()
            && m.a11y == A11yConformance::Declared;
        set.add("W02-四节-五节字段齐备", ok, "");
    }

    {
        // 激活点四类穷举（入口节完整性）。
        let ok = ActivationPoint::all().len() == 4;
        set.add("W02-四节-激活点四类穷举", ok, "");
    }

    // ---- 判据二：schema 校验（声明不实者加载期拒绝）----
    {
        let mut m = compliant_manifest();
        m.identity.id = String::new();
        let v = validate(&m);
        set.add(
            "W02-schema-身份缺失拒绝加载",
            !v.loadable && v.first_code() == "E_ID_EMPTY",
            "",
        );
    }

    {
        let mut m = compliant_manifest();
        m.compat.host_min = "9.0.0".to_string();
        m.compat.host_max = "1.0.0".to_string();
        let v = validate(&m);
        set.add(
            "W02-schema-兼容区间反转拒绝",
            !v.loadable && v.first_code() == "E_COMPAT_RANGE_INVERTED",
            "",
        );
    }

    {
        let mut m = compliant_manifest();
        m.entry.point = ActivationPoint::OnEvent;
        m.entry.argument = String::new();
        let v = validate(&m);
        set.add(
            "W02-schema-on-event缺参数拒绝",
            !v.loadable && v.first_code() == "E_ENTRY_ARG_MISSING",
            "",
        );
    }

    {
        let mut m = compliant_manifest();
        m.capability.requested = vec![Capability::Network, Capability::Network];
        let v = validate(&m);
        set.add(
            "W02-schema-能力重复声明拒绝",
            !v.loadable && v.first_code() == "E_CAPABILITY_DUPLICATE",
            "",
        );
    }

    {
        // 三要素齐备（发生了什么/为什么/下一步）——缺一即诊断不合格。
        let mut m = compliant_manifest();
        m.identity.name = String::new();
        let v = validate(&m);
        let three = v
            .diagnostics
            .iter()
            .all(|d| !d.what.is_empty() && !d.why.is_empty() && !d.next.is_empty());
        set.add("W02-schema-诊断三要素齐备", !v.loadable && three, "");
    }

    // ---- 判据三：最小缺省 ----
    {
        let mut m = compliant_manifest();
        m.capability.declared = false;
        m.capability.requested = Vec::new();
        let v = validate(&m);
        // 未声明能力节仍应准加载，但生效集必须是最小缺省（且不含网络/文件）。
        let safe = !v.effective.contains(&Capability::Network)
            && !v.effective.contains(&Capability::FileReadWrite);
        set.add(
            "W02-最小缺省-未声明走最小权限",
            v.loadable && safe && v.effective == Capability::MINIMAL_DEFAULT.to_vec(),
            "",
        );
    }

    {
        // 缺省方向是收敛不是便利：缺省集不得含任何 I/O 能力。
        let d = Capability::MINIMAL_DEFAULT;
        let ok = !d.contains(&Capability::FileReadWrite) && !d.contains(&Capability::Network);
        set.add("W02-最小缺省-缺省集不含IO能力", ok, "");
    }

    {
        // 显式声明宽权限时如实生效（校验器不擅自削减声明）。
        let m = compliant_manifest();
        let v = validate(&m);
        let honored = v.effective.contains(&Capability::DisplayColor)
            && v.effective.contains(&Capability::A11yGate);
        set.add("W02-最小缺省-声明权限如实生效", honored, "");
    }

    // ---- 判据四：升级提示 ----
    {
        let mut m = compliant_manifest();
        m.format_version = 99;
        let v = validate(&m);
        let has_upgrade = v
            .diagnostics
            .iter()
            .any(|d| d.code == "E_FORMAT_VERSION_UNSUPPORTED" && d.next.contains("升级"));
        set.add(
            "W02-升级-格式版本不识别明确提示升级",
            !v.loadable && has_upgrade,
            "",
        );
    }

    {
        let mut m = compliant_manifest();
        m.format_version = 0;
        let v = validate(&m);
        set.add(
            "W02-升级-版本0亦判不识别",
            !v.loadable && v.first_code() == "E_FORMAT_VERSION_UNSUPPORTED",
            "",
        );
    }

    {
        // 兼容判定：区间内/外两侧各验一次。
        let m = compliant_manifest();
        let inside = host_supported(&m, "2.5.0");
        let below = host_supported(&m, "1.9.9");
        let above = host_supported(&m, "3.0.1");
        set.add(
            "W02-兼容-宿主区间判定含边界",
            inside && !below && !above && host_supported(&m, "3.0.0"),
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compliant_manifest_loads() {
        let v = validate(&compliant_manifest());
        assert!(v.loadable, "合规清单须准加载：{:?}", v.diagnostics);
    }

    #[test]
    fn missing_capability_falls_back_to_minimal() {
        let mut m = compliant_manifest();
        m.capability.declared = false;
        m.capability.requested = vec![Capability::Network];
        let v = validate(&m);
        assert!(v.loadable, "未声明能力节不该阻断加载");
        assert_eq!(
            v.effective,
            Capability::MINIMAL_DEFAULT.to_vec(),
            "缺省方向须是收敛：声明的宽权限须被忽略"
        );
    }

    #[test]
    fn format_version_gates_loading() {
        let mut m = compliant_manifest();
        m.format_version = MANIFEST_FORMAT_VERSION + 1;
        let v = validate(&m);
        assert!(!v.loadable);
        assert_eq!(v.first_code(), "E_FORMAT_VERSION_UNSUPPORTED");
    }

    #[test]
    fn identity_empty_is_rejected() {
        let mut m = compliant_manifest();
        m.identity.id = String::new();
        assert!(!validate(&m).loadable);
    }

    #[test]
    fn version_cmp_orders_numerically() {
        // 数值比较而非字典序：字典序会把 "10.0.0" 判成小于 "9.0.0"。
        assert_eq!(version_cmp("2.0.0", "10.0.0"), core::cmp::Ordering::Less);
        assert_eq!(version_cmp("2.1", "2.1.0"), core::cmp::Ordering::Equal);
        assert_eq!(version_cmp("3.0.0", "2.9.9"), core::cmp::Ordering::Greater);
    }

    #[test]
    fn host_range_is_inclusive() {
        let m = compliant_manifest();
        assert!(host_supported(&m, "2.0.0"));
        assert!(host_supported(&m, "3.0.0"));
        assert!(!host_supported(&m, "1.999.999"));
    }

    #[test]
    fn multiple_errors_are_all_reported() {
        let mut m = compliant_manifest();
        m.identity.id = String::new();
        m.identity.author = String::new();
        m.compat.host_min = String::new();
        let v = validate(&m);
        // 零静默：多错须一次报全，不许只报首个就收工。
        assert!(v.diagnostics.len() >= 3, "多错须报全：{}", v.diagnostics.len());
    }

    #[test]
    fn effects_checks_all_green() {
        let set = run_vew02_checks();
        let (p, f) = set.tally();
        assert!(!set.truncated());
        assert!(set.all_passed(), "VE-F4602 红项：{}/{}", p, p + f);
    }
}
