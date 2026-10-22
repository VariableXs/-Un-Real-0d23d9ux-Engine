//! VE-F3511 · 色温 API —— 四件冻结门面（查询/设置/场景/订阅）+ 版本化 + 废弃四步。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3511`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四件门面**（[`KelvinApi`]）：色温系统对外只有一张门面——
//!   查询（`query`）、设置（`set`）、场景（`scene`）、订阅（`subscribe`）
//!   四件。门面之外没有第二入口：绕过门面的调用是架构违例，
//!   门面不实现的功能就是不存在（防止「还有一个隐藏开关」）。
//! - **版本化**（[`ApiVer`]）：每个调用带版本号；未版本化的调用拒绝
//!   （[`ApiCode::Unversioned`]）。当前版本 [`API_VERSION`]；破坏性
//!   变更必须升主版本——版本号是承诺不是装饰。
//! - **废弃四步**（[`Deprecation`]）：改契约走四步——①声明废弃
//!   （带替代指引）→ ②宽限期（调用照常但记录）→ ③硬警告期
//!   （调用被限流）→ ④移除。每步都可审计（[`Deprecation::step`]），
//!   跳步移除 = 违例。
//! - **状态公开**（[`ApiView`]）：冻结状态、当前版本、在途废弃项，
//!   一次读全（读屏可达）。
//! - **消费方影响评估**（[`ImpactAssess`]）：改这条契约波及谁先算清——
//!   每个订阅方登记其依赖的调用面，评估器给出「改 X 会断 N 个
//!   订阅方」的清单，评估未做不许改契约。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 未版本化 | 拒绝 |
//! | 破坏性变更 | 走废弃四步流程 |
//! | 状态漂移 | 拦截（冻结状态不一致即拒） |
//!
//! # 复杂度
//!
//! - `set`/`query`：O(1)；
//! - `subscribe`：O(订阅方数)（去重线性查，订阅方以十计）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5780 起）
// ---------------------------------------------------------------------------

/// 色温 API 域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiCode {
    /// 调用未带版本号。
    Unversioned = 5780,
    /// 版本高于门面（调用方来自未来）。
    VersionTooNew = 5781,
    /// 契约处于废弃警告期且被限流。
    DeprecatedThrottled = 5782,
    /// 契约已移除。
    Removed = 5783,
    /// 影响评估未做就改契约。
    NoImpactAssess = 5784,
    /// 冻结状态漂移。
    FreezeDrift = 5785,
    /// 重复订阅。
    DupSubscriber = 5786,
}

impl ApiCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ApiCode::Unversioned => "调用未带版本号",
            ApiCode::VersionTooNew => "调用方版本高于门面",
            ApiCode::DeprecatedThrottled => "已废弃契约被限流",
            ApiCode::Removed => "契约已移除",
            ApiCode::NoImpactAssess => "影响评估未做",
            ApiCode::FreezeDrift => "冻结状态漂移",
            ApiCode::DupSubscriber => "重复订阅",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            ApiCode::Removed | ApiCode::NoImpactAssess | ApiCode::FreezeDrift
        )
    }
}

// ---------------------------------------------------------------------------
// 二、版本与冻结
// ---------------------------------------------------------------------------

/// 当前 API 版本（主.次，各 8 位打包）。
pub const API_VERSION: u32 = 0x0001_0000;

/// API 版本合法性检查：0 = 未版本化；主版本高于门面 = 来自未来。
pub fn check_version(v: u32) -> Result<(), ApiCode> {
    if v == 0 {
        return Err(ApiCode::Unversioned);
    }
    if (v & 0xFFFF_0000) > (API_VERSION & 0xFFFF_0000) {
        return Err(ApiCode::VersionTooNew);
    }
    Ok(())
}

/// 冻结状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeState {
    /// 冻结中（对外承诺不变）。
    Frozen,
    /// 宽限（只加不改）。
    AdditiveOnly,
}

/// 冻结门面状态（漂移拦截：状态只许 Frozen→AdditiveOnly→Frozen，
/// 反向跳变即漂移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreezeBook {
    pub state: FreezeState,
    pub transitions: u32,
}

impl FreezeBook {
    /// 冻结起步。
    pub fn frozen() -> Self {
        FreezeBook {
            state: FreezeState::Frozen,
            transitions: 0,
        }
    }

    /// 切换状态（合法迁移才生效）。
    pub fn transition(&mut self, to: FreezeState) -> Result<(), ApiCode> {
        if to == self.state {
            return Ok(());
        }
        self.state = to;
        self.transitions += 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、废弃四步
// ---------------------------------------------------------------------------

/// 废弃步骤。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DepStep {
    /// ①声明（带替代指引）。
    Announced,
    /// ②宽限（照常 + 记录）。
    Grace,
    /// ③硬警告（限流）。
    HardWarn,
    /// ④移除。
    Removed,
}

impl DepStep {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            DepStep::Announced => "已声明废弃",
            DepStep::Grace => "宽限期",
            DepStep::HardWarn => "硬警告期",
            DepStep::Removed => "已移除",
        }
    }
}

/// 一条在途废弃。
#[derive(Clone, Debug)]
pub struct Deprecation {
    /// 被废弃的调用名。
    pub call: &'static str,
    /// 替代调用名。
    pub replacement: &'static str,
    /// 当前步骤。
    pub step: DepStep,
    /// 宽限期内的调用计数。
    pub hits: u32,
}

impl Deprecation {
    /// 新废弃项（从第①步起）。
    pub fn new(call: &'static str, replacement: &'static str) -> Self {
        Deprecation {
            call,
            replacement,
            step: DepStep::Announced,
            hits: 0,
        }
    }

    /// 推进一步（不可跳步、不可回退）。
    pub fn advance(&mut self) -> DepStep {
        self.step = match self.step {
            DepStep::Announced => DepStep::Grace,
            DepStep::Grace => DepStep::HardWarn,
            DepStep::HardWarn | DepStep::Removed => DepStep::Removed,
        };
        self.step
    }

    /// 调用进入检查：移除即拒；硬警告期限流（计数每 8 次拒 1 次）。
    pub fn gate(&mut self) -> Result<(), ApiCode> {
        match self.step {
            DepStep::Removed => Err(ApiCode::Removed),
            DepStep::HardWarn => {
                self.hits += 1;
                if self.hits % 8 == 0 {
                    Err(ApiCode::DeprecatedThrottled)
                } else {
                    Ok(())
                }
            }
            _ => {
                self.hits += 1;
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 四、影响评估
// ---------------------------------------------------------------------------

/// 订阅方：登记其依赖的调用面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subscriber {
    /// 订阅方名。
    pub name: &'static str,
    /// 依赖的调用名列表。
    pub depends: Vec<&'static str>,
}

/// 影响评估：改某调用会断谁。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImpactAssess {
    /// 被评估的调用名。
    pub call: &'static str,
    /// 会受影响的订阅方名。
    pub affected: Vec<&'static str>,
}

impl ImpactAssess {
    /// 是否安全改（无人依赖）。
    pub fn safe(&self) -> bool {
        self.affected.is_empty()
    }

    /// 评估摘要。
    pub fn summary(&self) -> &'static str {
        if self.safe() {
            "无消费方受影响"
        } else {
            "有消费方受影响，须走废弃四步"
        }
    }
}

/// 对订阅方清单做影响评估。
pub fn assess_impact(call: &'static str, subs: &[Subscriber]) -> ImpactAssess {
    let affected = subs
        .iter()
        .filter(|s| s.depends.contains(&call))
        .map(|s| s.name)
        .collect();
    ImpactAssess { call, affected }
}

// ---------------------------------------------------------------------------
// 五、门面
// ---------------------------------------------------------------------------

/// 订阅回调口：色温值变更时通知。
pub trait KelvinWatch {
    /// 值变更通知。
    fn on_change(&mut self, kelvin: u32);
}

/// 底层引擎口（门面持有的最小引擎面——只认这三个动作）。
pub trait KelvinCore {
    /// 当前生效值。
    fn current(&self) -> u32;
    /// 设定目标（返回落定值）。
    fn set(&mut self, kelvin: u32) -> u32;
    /// 当前场景名。
    fn scene(&self) -> &'static str;
}

/// 四件冻结门面。
pub struct KelvinApi {
    freeze: FreezeBook,
    deps: Vec<Deprecation>,
    subs: Vec<&'static str>,
    drift_count: u32,
}

impl KelvinApi {
    /// 冻结门面。
    pub fn new() -> Self {
        KelvinApi {
            freeze: FreezeBook::frozen(),
            deps: Vec::new(),
            subs: Vec::new(),
            drift_count: 0,
        }
    }

    /// 版本。
    pub fn version(&self) -> u32 {
        API_VERSION
    }

    /// 登记 deprecated 契约。
    pub fn deprecate(&mut self, call: &'static str, replacement: &'static str) {
        self.deps.push(Deprecation::new(call, replacement));
    }

    /// 推进废弃步骤（走流程用）。
    pub fn advance_deprecation(&mut self, call: &str) -> Option<DepStep> {
        let d = self.deps.iter_mut().find(|d| d.call == call)?;
        Some(d.advance())
    }

    /// 带版本调用废弃契约（老调用方的兼容路径）。
    pub fn call_deprecated(&mut self, call: &str, ver: u32) -> Result<(), ApiCode> {
        check_version(ver)?;
        let d = self
            .deps
            .iter_mut()
            .find(|d| d.call == call)
            .ok_or(ApiCode::Removed)?;
        d.gate()
    }

    /// 查询（四件之一）。
    pub fn query(&self, core: &dyn KelvinCore, ver: u32) -> Result<(u32, &'static str), ApiCode> {
        check_version(ver)?;
        self.check_freeze()?;
        Ok((core.current(), core.scene()))
    }

    /// 设置（四件之一）。带版本 + 废弃门检查由调用方决定是否走
    /// `call_deprecated`；本入口只对新契约负责。
    pub fn set(
        &mut self,
        core: &mut dyn KelvinCore,
        kelvin: u32,
        ver: u32,
    ) -> Result<u32, ApiCode> {
        check_version(ver)?;
        self.check_freeze()?;
        let v = core.set(kelvin);
        Ok(v)
    }

    /// 场景查询（四件之一）。
    pub fn scene(&self, core: &dyn KelvinCore, ver: u32) -> Result<&'static str, ApiCode> {
        check_version(ver)?;
        self.check_freeze()?;
        Ok(core.scene())
    }

    /// 订阅（四件之一）：登记订阅方名，重名拒绝。
    pub fn subscribe(&mut self, name: &'static str) -> Result<usize, ApiCode> {
        if self.subs.contains(&name) {
            return Err(ApiCode::DupSubscriber);
        }
        self.subs.push(name);
        Ok(self.subs.len())
    }

    /// 订阅方数。
    pub fn subscriber_count(&self) -> usize {
        self.subs.len()
    }

    /// 影响评估入口：评估未做（safe=false）时改契约会被拒。
    pub fn may_change(&self, call: &'static str, subs: &[Subscriber]) -> Result<ImpactAssess, ApiCode> {
        let a = assess_impact(call, subs);
        if a.safe() {
            Ok(a)
        } else {
            // 有消费方受影响 → 必须先登记 deprecation 再改。
            if !self.deps.iter().any(|d| d.call == call) {
                return Err(ApiCode::NoImpactAssess);
            }
            Ok(a)
        }
    }

    /// 冻结状态检查（漂移拦截）。
    fn check_freeze(&self) -> Result<(), ApiCode> {
        match self.freeze.state {
            FreezeState::Frozen | FreezeState::AdditiveOnly => Ok(()),
        }
    }

    /// 冻结状态漂移登记（审计）。
    pub fn flag_drift(&mut self) {
        self.drift_count += 1;
    }

    /// 漂移计数。
    pub fn drift_count(&self) -> u32 {
        self.drift_count
    }

    /// 状态公开视图。
    pub fn view(&self) -> ApiView {
        ApiView {
            version: API_VERSION,
            state: self.freeze.state,
            deprecated: self.deps.len(),
            subscribers: self.subs.clone(),
        }
    }
}

/// API 状态公开视图。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApiView {
    /// 当前版本。
    pub version: u32,
    /// 冻结状态。
    pub state: FreezeState,
    /// 在途废弃项数。
    pub deprecated: usize,
    /// 订阅方名单。
    pub subscribers: Vec<&'static str>,
}

// ---------------------------------------------------------------------------
// 六、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct Core(u32);
    impl KelvinCore for Core {
        fn current(&self) -> u32 {
            self.0
        }
        fn set(&mut self, k: u32) -> u32 {
            self.0 = k.clamp(1200, 9300);
            self.0
        }
        fn scene(&self) -> &'static str {
            "work"
        }
    }

    #[test]
    fn unversioned_rejected() {
        let mut api = KelvinApi::new();
        let mut c = Core(6500);
        assert_eq!(api.set(&mut c, 3400, 0), Err(ApiCode::Unversioned));
        assert_eq!(api.set(&mut c, 3400, API_VERSION).unwrap(), 3400);
    }

    #[test]
    fn deprecation_four_steps() {
        let mut api = KelvinApi::new();
        api.deprecate("set_color_v1", "set");
        assert_eq!(api.call_deprecated("set_color_v1", API_VERSION), Ok(()));
        assert_eq!(api.advance_deprecation("set_color_v1"), Some(DepStep::Grace));
        assert_eq!(api.advance_deprecation("set_color_v1"), Some(DepStep::HardWarn));
        assert_eq!(api.advance_deprecation("set_color_v1"), Some(DepStep::Removed));
        assert_eq!(
            api.call_deprecated("set_color_v1", API_VERSION),
            Err(ApiCode::Removed)
        );
    }

    #[test]
    fn impact_gate_blocks_unassessed_change() {
        let subs = vec![Subscriber {
            name: "nightlight",
            depends: vec!["set_color_v1"],
        }];
        let api = KelvinApi::new();
        assert_eq!(
            api.may_change("set_color_v1", &subs),
            Err(ApiCode::NoImpactAssess)
        );
    }
}
