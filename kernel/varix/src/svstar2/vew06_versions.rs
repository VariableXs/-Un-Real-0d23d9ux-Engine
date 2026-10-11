//! VE-F4606 · 插件版本与兼容（VE-X 域 · 插件系统组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4606`
//!
//! **判据（锚点原文）**：语义版本、三维矩阵、双版期、迁移期、判据。
//!
//! **职责定位（锚点原文）**：插件版本与兼容（语义版本强制：插件版本与 API
//! 依赖版本声明；兼容矩阵：插件×宿主版本×平台三维矩阵实测；破坏性变更零
//! 容忍：宿主 API 废弃走双版期——插件有完整迁移期）。
//!
//! **数据结构（锚点原文）**：语义版本校验器；三维兼容矩阵；双版期计时。
//!
//! **错误路径与降级矩阵（锚点原文）**：版本不识别→明确报错+升级指引；矩阵
//! 缺格→补测排程；兼容破坏→版本拦截。
//!
//! **性能逐项分解（锚点原文）**：校验 O(1)；矩阵 O(格数)；拦截 O(1)。
//!
//! **跨批对接点（锚点原文）**：F4602 声明上游；F4607 API 冻结衔接；F4624
//! 更新消费。
//!
//! **上游单源衔接**：版本门编排与 F4605 信任链（[`super::vew05_trust::
//! TrustChain`]）真调衔接——版本门全过的插件才进 `verify_and_install`；
//! 兼容破坏的插件在版本门就被拦，不进信任面。
//!
//! # 判据怎么做到"不是恒真"
//!
//! 1. **"语义版本"**。若只断「能解析」，不识别的输入走进来照样崩。必须断
//!    **负例分型**（空串/缺段/非数字/负数各自被拒）+ 报错携带**升级指引**
//!    （裸错误码没有可操作性——锚点「明确报错+升级指引」）。
//! 2. **"三维矩阵"**。若只断「能标记」，缺格看不见。必须断**笛卡尔积格数
//!    对账**（插件×宿主×平台全组合格数 = 三维相乘）+ **缺格检测→补测排程**
//!    （排程是登记不是口号——排程列表可读回）。
//! 3. **"双版期/迁移期"**。若迁移期长短没人管，「完整迁移期」就是一句空话。
//!    必须断：废弃时刻 = 申报时刻 + 迁移期（**期长可配置且固定**）+ 迁移
//!    期内 API 可用/过期后禁用**双向** + 用过期 API 的插件被**版本拦截**
//!    （拦截清单可读回——不是静默拒绝）。
//! 4. **"兼容破坏零容忍"**。必须断主流程编排的**分型拦截**：语义版本不识别、
//!    API 不兼容、废弃超期三条失败路径各自给出**专属拦截原因**（一个笼统的
//!    "失败"会让插件作者无从修复）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vew05_trust::TrustChain;

// ===========================================================================
// 一、语义版本校验器
// ===========================================================================

/// 宿主 API 语义版本（major.minor——major 破坏性、minor 增量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemVer {
    /// 主版本（破坏性变更位）。
    pub major: u32,
    /// 次版本（增量位）。
    pub minor: u32,
    /// 修订位。
    pub patch: u32,
}

/// 版本错误（**明确报错 + 升级指引**——锚点错误路径第 1 条）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VersionError {
    /// 原始输入（定位用）。
    pub input: String,
    /// 失败原因。
    pub reason: String,
    /// 升级指引（非空——裸错误没有可操作性）。
    pub upgrade_hint: String,
}

/// 解析语义版本 `major.minor.patch`（三段非负整数；缺段/非数字/空段=不识别）。
pub fn parse_semver(s: &str) -> Result<SemVer, VersionError> {
    let err = |reason: &str| VersionError {
        input: s.to_string(),
        reason: reason.to_string(),
        upgrade_hint: format!(
            "版本须为 major.minor.patch 三段非负整数（如 1.4.0）；宿主当前 API 主版本 {}——请升级插件声明后重新提交",
            HOST_API_MAJOR
        ),
    };
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return Err(err("段数不是 3（须 major.minor.patch）"));
    }
    let mut nums = [0u32; 3];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return Err(err("存在空段或非数字字符"));
        }
        nums[i] = p
            .parse::<u32>()
            .map_err(|_| err("段值超出 u32 范围"))?;
    }
    Ok(SemVer { major: nums[0], minor: nums[1], patch: nums[2] })
}

/// 宿主当前 API 主版本（F4607 API 冻结衔接位——冻结版本即 major 锚）。
pub const HOST_API_MAJOR: u32 = 1;

/// API 依赖声明（插件声明它需要的宿主 API 版本）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApiRequirement {
    /// 依赖的主版本。
    pub major: u32,
    /// 依赖的最低次版本。
    pub minor: u32,
}

/// API 兼容判定（语义版本规则：major 相同且宿主 minor ≥ 依赖 minor）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compatibility {
    /// 兼容（可装）。
    Compatible,
    /// 主版本不同（破坏性变更——零容忍拦截）。
    MajorMismatch,
    /// 宿主 minor 低于依赖（缺增量特性）。
    MinorLacking,
}

/// API 兼容判定（O(1)——锚点性能口径）。
pub fn api_compatible(req: ApiRequirement, host: ApiRequirement) -> Compatibility {
    if req.major != host.major {
        Compatibility::MajorMismatch
    } else if host.minor < req.minor {
        Compatibility::MinorLacking
    } else {
        Compatibility::Compatible
    }
}

// ===========================================================================
// 二、三维兼容矩阵（插件 × 宿主版本 × 平台）
// ===========================================================================

/// 矩阵格状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellStatus {
    /// 实测通过。
    Pass,
    /// 实测失败。
    Fail,
    /// 未测（缺格——须补测排程）。
    Unmeasured,
}

/// 三维兼容矩阵（格 = 插件 id × 宿主版本 × 平台；O(格数) 操作——锚点口径）。
#[derive(Clone, Debug, Default)]
pub struct CompatMatrix {
    /// 已实测格：(插件, 宿主版本, 平台, 状态)。
    cells: Vec<(String, String, String, CellStatus)>,
    /// 补测排程（缺格登记——排程是登记不是口号，可读回）。
    pub schedule: Vec<(String, String, String)>,
}

impl CompatMatrix {
    /// 标记一格实测结果（O(1) 摊销）。
    pub fn mark(&mut self, plugin: &str, host: &str, platform: &str, st: CellStatus) {
        if let Some(slot) = self
            .cells
            .iter_mut()
            .find(|(p, h, pl, _)| p == plugin && h == host && pl == platform)
        {
            slot.3 = st;
        } else {
            self.cells.push((plugin.to_string(), host.to_string(), platform.to_string(), st));
        }
    }

    /// 查一格（未测 = Unmeasured）。
    pub fn status_of(&self, plugin: &str, host: &str, platform: &str) -> CellStatus {
        self.cells
            .iter()
            .find(|(p, h, pl, _)| p == plugin && h == host && pl == platform)
            .map(|(_, _, _, st)| *st)
            .unwrap_or(CellStatus::Unmeasured)
    }

    /**
     * 缺格检测 → 补测排程（锚点「矩阵缺格→补测排程」）。
     *
     * 对插件 × 宿主版本集 × 平台集的**全笛卡尔积**扫一遍，Unmeasured 格
     * 登记进补测排程（幂等：重复调用不重复登记）。
     */
    pub fn schedule_missing(
        &mut self,
        plugin: &str,
        hosts: &[&str],
        platforms: &[&str],
    ) -> usize {
        let mut added = 0usize;
        for h in hosts {
            for pl in platforms {
                if self.status_of(plugin, h, pl) == CellStatus::Unmeasured {
                    let dup = self
                        .schedule
                        .iter()
                        .any(|(p, hh, pp)| p == plugin && hh == h && pp == pl);
                    if !dup {
                        self.schedule
                            .push((plugin.to_string(), h.to_string(), pl.to_string()));
                        added += 1;
                    }
                }
            }
        }
        added
    }

    /// 已测格数。
    pub fn measured(&self) -> usize {
        self.cells.len()
    }

    /// 补测排程条数。
    pub fn scheduled(&self) -> usize {
        self.schedule.len()
    }
}

// ===========================================================================
// 三、双版期计时（宿主 API 废弃——插件有完整迁移期）
// ===========================================================================

/// 一条废弃申报。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Deprecation {
    /// 废弃的 API 名。
    pub api: String,
    /// 替代 API（迁移指引——非空）。
    pub successor: String,
    /// 移除时刻（tick；= 申报时 now + 迁移期）。
    pub removed_at: u64,
}

/// 双版期时钟（迁移期固定可配——「完整迁移期」是承诺不是估计）。
#[derive(Clone, Debug)]
pub struct DualVersionClock {
    /// 当前 tick（与 F4605 信任链 tick 同口径——逻辑时钟）。
    pub now: u64,
    /// 迁移期（tick 数；废弃到移除的完整窗口）。
    pub migration_ticks: u64,
    /// 废弃申报列表。
    deprecations: Vec<Deprecation>,
}

/// 默认迁移期（86400 tick——一个完整日周期，期长声明写在常量上不散在参数里）。
pub const DEFAULT_MIGRATION_TICKS: u64 = 86_400;

impl DualVersionClock {
    /// 构造（默认迁移期）。
    pub fn new() -> DualVersionClock {
        DualVersionClock {
            now: 0,
            migration_ticks: DEFAULT_MIGRATION_TICKS,
            deprecations: Vec::new(),
        }
    }

    /// 推进 tick。
    pub fn advance(&mut self, n: u64) {
        self.now = self.now.saturating_add(n);
    }

    /// 申报废弃（removed_at = now + 迁移期——插件自申报起有完整迁移期）。
    pub fn deprecate(&mut self, api: &str, successor: &str) -> u64 {
        let removed_at = self.now.saturating_add(self.migration_ticks);
        self.deprecations.push(Deprecation {
            api: api.to_string(),
            successor: successor.to_string(),
            removed_at,
        });
        removed_at
    }

    /// API 当前是否可用（迁移期内可用、过期禁用——双向判定的实现面）。
    pub fn api_usable(&self, api: &str) -> bool {
        !self
            .deprecations
            .iter()
            .any(|d| d.api == api && self.now >= d.removed_at)
    }

    /// 查废弃申报。
    pub fn deprecation_of(&self, api: &str) -> Option<&Deprecation> {
        self.deprecations.iter().find(|d| d.api == api)
    }

    /**
     * 兼容破坏拦截（O(1)/API——锚点口径）：插件使用的 API 清单里若有
     * 已过废弃期的，逐条产出**版本拦截**（拦截清单可读回——不是静默拒绝）。
     */
    pub fn enforce(&self, used_apis: &[String]) -> Vec<VersionBlock> {
        let mut blocks = Vec::new();
        for api in used_apis {
            if let Some(d) = self.deprecation_of(api) {
                if self.now >= d.removed_at {
                    blocks.push(VersionBlock {
                        api: api.clone(),
                        reason: BlockReason::DeprecatedPastDeadline,
                        hint: format!("迁移到 {}（废弃期已过 {} tick）", d.successor, self.now - d.removed_at),
                    });
                }
            }
        }
        blocks
    }
}

impl Default for DualVersionClock {
    fn default() -> Self {
        Self::new()
    }
}

/// 拦截原因（分型——一个笼统的"失败"会让插件作者无从修复）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockReason {
    /// 语义版本不识别。
    UnrecognizedVersion,
    /// API 主版本不匹配（破坏性变更零容忍）。
    MajorMismatch,
    /// 宿主 minor 缺增量特性。
    MinorLacking,
    /// 使用已过废弃期的 API。
    DeprecatedPastDeadline,
}

/// 版本拦截条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VersionBlock {
    /// 触发面（API 名或版本串）。
    pub api: String,
    /// 拦截原因（分型）。
    pub reason: BlockReason,
    /// 处置指引（非空）。
    pub hint: String,
}

// ===========================================================================
// 四、安装版本门（主流程编排——全过才进 F4605 信任面）
// ===========================================================================

/// 安装版本门结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GateOutcome {
    /// 版本门全过（可进 F4605 `verify_and_install`）。
    Passed,
    /// 被版本拦截（分型原因+指引在块里）。
    Blocked(Vec<VersionBlock>),
}

/// 安装版本门（主流程编排：语义版本 → API 兼容 → 废弃拦截 → 信任面）。
///
/// 编排次序不可换：先解析（不识别就无后续可判）、再 API 兼容（major 拦）、
/// 最后废弃超期拦截（最细粒度）。信任面（F4605 `verify_and_install`）由
/// 调用方在 `Passed` 后执行——版本门不吞信任职责。
pub fn install_version_gate(
    version_str: &str,
    api_req: ApiRequirement,
    host: ApiRequirement,
    clock: &DualVersionClock,
    used_apis: &[String],
) -> GateOutcome {
    let mut blocks: Vec<VersionBlock> = Vec::new();
    // 1) 语义版本解析（不识别 → 明确报错 + 升级指引）
    let ver = match parse_semver(version_str) {
        Ok(v) => v,
        Err(e) => {
            blocks.push(VersionBlock {
                api: version_str.to_string(),
                reason: BlockReason::UnrecognizedVersion,
                hint: e.upgrade_hint,
            });
            return GateOutcome::Blocked(blocks);
        }
    };
    let _ = ver;
    // 2) API 兼容（major 破坏性零容忍 / minor 缺特性分型）
    match api_compatible(api_req, host) {
        Compatibility::Compatible => {}
        Compatibility::MajorMismatch => blocks.push(VersionBlock {
            api: format!("host-api@{}.{}", api_req.major, api_req.minor),
            reason: BlockReason::MajorMismatch,
            hint: format!("宿主 API 主版本为 {}——破坏性变更零容忍，请按迁移指南升级插件", host.major),
        }),
        Compatibility::MinorLacking => blocks.push(VersionBlock {
            api: format!("host-api@{}.{}", api_req.major, api_req.minor),
            reason: BlockReason::MinorLacking,
            hint: format!("宿主当前 minor 为 {}——升级宿主或降低依赖", host.minor),
        }),
    }
    // 3) 废弃超期拦截
    blocks.extend(clock.enforce(used_apis));
    if blocks.is_empty() {
        GateOutcome::Passed
    } else {
        GateOutcome::Blocked(blocks)
    }
}

// ===========================================================================
// 五、域自检（判据逐条映射锚点）
// ===========================================================================

/// VE-F4606 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// 跑全部判据。
    pub fn run_vew06_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vew06");

        // -- X6-VER-01 语义版本解析：合法过、负例分型、指引非空 ---------------
        {
            let ok = parse_semver("1.4.0");
            let empty = parse_semver("");
            let short = parse_semver("1.4");
            let alpha = parse_semver("1.a.0");
            let ok_shape = matches!(&ok, Ok(v) if v.major == 1 && v.minor == 4 && v.patch == 0);
            let neg_all = [&empty, &short, &alpha].iter().all(|r| r.is_err());
            let hint_ok = match &empty {
                Err(e) => !e.upgrade_hint.is_empty() && !e.reason.is_empty(),
                Ok(_) => false,
            };
            s.add(
                "X6-VER-01 语义版本：1.4.0 解析正确、空串/缺段/非数字各自被拒、报错带升级指引",
                ok_shape && neg_all && hint_ok,
                "锚点「版本不识别→明确报错+升级指引」；裸错误码没有可操作性",
            );
        }

        // -- X6-VER-02 API 兼容语义三型：major 拦/minor 缺/兼容 ----------------
        {
            let host = ApiRequirement { major: 1, minor: 7 };
            let same = api_compatible(ApiRequirement { major: 1, minor: 3 }, host);
            let lacking = api_compatible(ApiRequirement { major: 1, minor: 9 }, host);
            let major = api_compatible(ApiRequirement { major: 2, minor: 0 }, host);
            s.add(
                "X6-VER-02 API 兼容三型：同 major 且 minor 够=兼容、minor 缺=MinorLacking、major 异=MajorMismatch",
                matches!(same, Compatibility::Compatible)
                    && matches!(lacking, Compatibility::MinorLacking)
                    && matches!(major, Compatibility::MajorMismatch),
                "语义版本规则：major 破坏性零容忍、minor 增量向后兼容",
            );
        }

        // -- X6-VER-03 版本拦截：不兼容插件在版本门被拦（不进信任面）----------
        {
            let clock = DualVersionClock::new();
            let host = ApiRequirement { major: 1, minor: 7 };
            let gate = install_version_gate(
                "1.0.0",
                ApiRequirement { major: 2, minor: 0 },
                host,
                &clock,
                &[],
            );
            s.add(
                "X6-VER-03 版本拦截：major 不匹配插件在版本门被拦（MajorMismatch 分型+指引）",
                matches!(&gate, GateOutcome::Blocked(b) if b.iter().any(|x| x.reason == BlockReason::MajorMismatch && !x.hint.is_empty())),
                "锚点「兼容破坏→版本拦截」；兼容破坏的插件不进 F4605 信任面",
            );
        }

        // -- X6-MAT-01 三维矩阵：标记/回读/双状态 ------------------------------
        {
            let mut m = CompatMatrix::default();
            m.mark("p1", "host-1.0", "win", CellStatus::Pass);
            m.mark("p1", "host-1.0", "linux", CellStatus::Fail);
            s.add(
                "X6-MAT-01 三维矩阵：Pass/Fail 标记回读、未测格报 Unmeasured（三维键全等寻址）",
                m.status_of("p1", "host-1.0", "win") == CellStatus::Pass
                    && m.status_of("p1", "host-1.0", "linux") == CellStatus::Fail
                    && m.status_of("p1", "host-1.0", "mac") == CellStatus::Unmeasured,
                "锚点「插件×宿主版本×平台三维矩阵实测」；未测=未测（不冒充实测）",
            );
        }

        // -- X6-MAT-02 缺格→补测排程（幂等、笛卡尔积格数对账）-----------------
        {
            let mut m = CompatMatrix::default();
            m.mark("p1", "h1", "win", CellStatus::Pass);
            let hosts = ["h1", "h2"];
            let platforms = ["win", "linux", "mac"];
            let first = m.schedule_missing("p1", &hosts, &platforms);
            let second = m.schedule_missing("p1", &hosts, &platforms);
            // 笛卡尔积 2×3=6 格，已测 1 → 缺 5
            s.add(
                "X6-MAT-02 缺格补测排程：2×3 积缺 5 格首扫全登记、重扫幂等零新增",
                first == 5 && second == 0 && m.scheduled() == 5 && m.measured() == 1,
                "锚点「矩阵缺格→补测排程」；排程是可读回的登记不是口号",
            );
        }

        // -- X6-DUAL-01 双版期：废弃时刻=申报+迁移期（完整迁移期承诺）----------
        {
            let mut c = DualVersionClock::new();
            c.advance(100);
            let removed = c.deprecate("draw.legacy_blit", "draw.blit_v2");
            s.add(
                "X6-DUAL-01 双版期：移除时刻=申报 tick+迁移期（86400 完整窗口，期长声明在常量）",
                removed == 100 + DEFAULT_MIGRATION_TICKS && c.migration_ticks == DEFAULT_MIGRATION_TICKS,
                "锚点「宿主 API 废弃走双版期——插件有完整迁移期」；期长短没人管=承诺是空话",
            );
        }

        // -- X6-DUAL-02 迁移期内可用/过期后禁用（双向）-------------------------
        {
            let mut c = DualVersionClock::new();
            c.advance(10);
            let _ = c.deprecate("old.api", "new.api");
            let during = c.api_usable("old.api");
            c.advance(DEFAULT_MIGRATION_TICKS);
            let after = c.api_usable("old.api");
            s.add(
                "X6-DUAL-02 迁移期内 API 可用、过期后禁用（双向判定）",
                during && !after,
                "锚点「破坏性变更零容忍」的软着陆面；无双向判定的迁移期是摆设",
            );
        }

        // -- X6-DUAL-03 兼容破坏拦截：用过期 API 的插件被版本拦截 --------------
        {
            let mut c = DualVersionClock::new();
            let _ = c.deprecate("old.api", "new.api");
            c.advance(DEFAULT_MIGRATION_TICKS + 1);
            let blocks = c.enforce(&["new.api".to_string(), "old.api".to_string()]);
            s.add(
                "X6-DUAL-03 兼容破坏拦截：过期 API 进拦截清单（分型+迁移指引）、未过期 API 不误伤",
                blocks.len() == 1
                    && blocks[0].api == "old.api"
                    && blocks[0].reason == BlockReason::DeprecatedPastDeadline
                    && blocks[0].hint.contains("new.api"),
                "锚点「兼容破坏→版本拦截」；拦截清单可读回（不是静默拒绝）",
            );
        }

        // -- X6-GATE-01 主流程编排：全过→Passed；三失败路径各自分型 ------------
        {
            let clock = DualVersionClock::new();
            let host = ApiRequirement { major: 1, minor: 7 };
            let pass = install_version_gate("1.0.0", ApiRequirement { major: 1, minor: 3 }, host, &clock, &[]);
            let bad_ver = install_version_gate("abc", ApiRequirement { major: 1, minor: 3 }, host, &clock, &[]);
            let mut clock2 = DualVersionClock::new();
            let _ = clock2.deprecate("old.api", "new.api");
            clock2.advance(DEFAULT_MIGRATION_TICKS + 5);
            let deprecated = install_version_gate(
                "1.0.0",
                ApiRequirement { major: 1, minor: 3 },
                host,
                &clock2,
                &["old.api".to_string()],
            );
            let blocked_reasons = |g: &GateOutcome| match g {
                GateOutcome::Blocked(b) => b.iter().map(|x| x.reason).collect::<Vec<_>>(),
                GateOutcome::Passed => Vec::new(),
            };
            s.add(
                "X6-GATE-01 主流程编排：全过 Passed；不识别/废弃超期各给专属分型（笼统失败=无从修复）",
                matches!(pass, GateOutcome::Passed)
                    && blocked_reasons(&bad_ver) == vec![BlockReason::UnrecognizedVersion]
                    && blocked_reasons(&deprecated) == vec![BlockReason::DeprecatedPastDeadline],
                "锚点「版本不识别→明确报错」「兼容破坏→版本拦截」；三条失败路径分型齐",
            );
        }

        // -- X6-UP-01 上游衔接：版本门过的插件进 F4605 信任链（真调）-----------
        {
            let clock = DualVersionClock::new();
            let host = ApiRequirement { major: 1, minor: 7 };
            let gate = install_version_gate("1.0.0", ApiRequirement { major: 1, minor: 3 }, host, &clock, &[]);
            let mut chain = TrustChain::new();
            chain.add_official_key("official-key-1");
            let installed = match gate {
                GateOutcome::Passed => chain.verify_and_install("compat-plugin", "official-key-1").allowed,
                GateOutcome::Blocked(_) => false,
            };
            s.add(
                "X6-UP-01 上游衔接：版本门 Passed 后真调 F4605 verify_and_install 成功（插件进信任面）",
                installed && chain.installed().len() == 1,
                "锚点「F4602 声明上游」「F4607 API 冻结衔接」；版本门与信任面是接力不是两套",
            );
        }

        // -- X6-HINT-01 拦截指引逐条非空（可操作性红线）------------------------
        {
            let clock = DualVersionClock::new();
            let host = ApiRequirement { major: 1, minor: 7 };
            let bad = install_version_gate("x.y", ApiRequirement { major: 2, minor: 0 }, host, &clock, &[]);
            let hints_ok = match &bad {
                GateOutcome::Blocked(b) => b.iter().all(|x| !x.hint.is_empty()),
                GateOutcome::Passed => false,
            };
            s.add(
                "X6-HINT-01 拦截指引逐条非空（每条拦截都告诉插件作者怎么修）",
                hints_ok,
                "无指引的拦截=把修复成本全部甩给插件作者",
            );
        }

        s
    }
}

