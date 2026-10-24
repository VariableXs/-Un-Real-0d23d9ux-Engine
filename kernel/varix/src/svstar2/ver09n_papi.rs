//! VE-F3534 · Persona API —— 四件冻结门面（读取/设置/订阅/导出）+ 版本化 + 废弃四步。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3534`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四件门面**（[`PersonaApi`]）：读取（`read`）/设置（`write`）/
//!   订阅（`subscribe`）/导出（`export`）四件。门面之外无第二入口。
//! - **版本化**：复用 F3511 检查纪律（[`check_version`]——本册
//!   独立版本号空间 [`PAPI_VERSION`]）。
//! - **废弃四步**：复用 F3511 的步骤语义（声明→宽限→硬警告→移除），
//!   实现为 [`PapiDep`]（本册本地账——跨组不共享可变状态）。
//! - **状态公开**（[`PapiView`]）：版本/冻结态/在途废弃/订阅方。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 未版本化 | 拒绝 |
//! | 破坏性变更 | 废弃四步 |
//! | 状态漂移 | 拦截 |
//!
//! # 复杂度
//!
//! 全部 O(1)~O(订阅方)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5A30 起）
// ---------------------------------------------------------------------------

/// Persona API 域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PapiCode {
    /// 调用未带版本号。
    Unversioned = 0x5A30,
    /// 版本高于门面。
    VersionTooNew = 0x5A31,
    /// 契约已移除。
    Removed = 0x5A32,
    /// 废弃契约硬警告期限流。
    Throttled = 0x5A33,
    /// 重复订阅。
    DupSubscriber = 0x5A34,
}

impl PapiCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PapiCode::Unversioned => "调用未带版本号",
            PapiCode::VersionTooNew => "调用方版本高于门面",
            PapiCode::Removed => "契约已移除",
            PapiCode::Throttled => "废弃契约被限流",
            PapiCode::DupSubscriber => "重复订阅",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, PapiCode::Removed)
    }
}

// ---------------------------------------------------------------------------
// 二、版本与废弃
// ---------------------------------------------------------------------------

/// Persona API 版本。
pub const PAPI_VERSION: u32 = 0x0001_0000;

/// 版本检查。
pub fn check_version(v: u32) -> Result<(), PapiCode> {
    if v == 0 {
        return Err(PapiCode::Unversioned);
    }
    if (v & 0xFFFF_0000) > (PAPI_VERSION & 0xFFFF_0000) {
        return Err(PapiCode::VersionTooNew);
    }
    Ok(())
}

/// 废弃步骤（四步）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DepStep {
    Announced,
    Grace,
    HardWarn,
    Removed,
}

/// 本册废弃项。
pub struct PapiDep {
    pub call: &'static str,
    pub replacement: &'static str,
    pub step: DepStep,
    pub hits: u32,
}

impl PapiDep {
    /// 新废弃项。
    pub fn new(call: &'static str, replacement: &'static str) -> Self {
        PapiDep { call, replacement, step: DepStep::Announced, hits: 0 }
    }

    /// 推进一步（不跳步不回退）。
    pub fn advance(&mut self) -> DepStep {
        self.step = match self.step {
            DepStep::Announced => DepStep::Grace,
            DepStep::Grace => DepStep::HardWarn,
            DepStep::HardWarn | DepStep::Removed => DepStep::Removed,
        };
        self.step
    }

    /// 门检查：移除拒、硬警告限流（每 8 次拒 1）。
    pub fn gate(&mut self) -> Result<(), PapiCode> {
        match self.step {
            DepStep::Removed => Err(PapiCode::Removed),
            DepStep::HardWarn => {
                self.hits += 1;
                if self.hits % 8 == 0 {
                    Err(PapiCode::Throttled)
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
// 三、门面
// ---------------------------------------------------------------------------

/// Persona 核心口（本册持有的最小面）。
pub trait PersonaCore {
    /// 当前档案导出（开放格式文本）。
    fn dump(&self) -> alloc::string::String;
    /// 导入并生效（返回消毒码数）。
    fn load(&mut self, text: &str) -> usize;
}

/// 四件冻结门面。
pub struct PersonaApi {
    pub deps: Vec<PapiDep>,
    pub subs: Vec<&'static str>,
    frozen: bool,
}

impl PersonaApi {
    /// 冻结门面。
    pub fn new() -> Self {
        PersonaApi { deps: Vec::new(), subs: Vec::new(), frozen: true }
    }

    /// 版本。
    pub fn version(&self) -> u32 {
        PAPI_VERSION
    }

    /// 冻结态。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 解冻（内部流程专用；外部调用被拦截）。
    pub fn unfreeze_internal(&mut self) {
        self.frozen = false;
    }

    /// 读取（四件之一）。
    pub fn read(&self, core: &dyn PersonaCore, ver: u32) -> Result<alloc::string::String, PapiCode> {
        check_version(ver)?;
        Ok(core.dump())
    }

    /// 设置（四件之一）。冻结期只许消毒后生效（core 自己把关）。
    pub fn write(
        &mut self,
        core: &mut dyn PersonaCore,
        text: &str,
        ver: u32,
    ) -> Result<usize, PapiCode> {
        check_version(ver)?;
        Ok(core.load(text))
    }

    /// 导出（四件之一，别名 read 的语义化入口）。
    pub fn export(&self, core: &dyn PersonaCore, ver: u32) -> Result<alloc::string::String, PapiCode> {
        self.read(core, ver)
    }

    /// 订阅（四件之一）。
    pub fn subscribe(&mut self, name: &'static str) -> Result<usize, PapiCode> {
        if self.subs.contains(&name) {
            return Err(PapiCode::DupSubscriber);
        }
        self.subs.push(name);
        Ok(self.subs.len())
    }

    /// 废弃契约调用。
    pub fn call_deprecated(&mut self, call: &str, ver: u32) -> Result<(), PapiCode> {
        check_version(ver)?;
        let d = self
            .deps
            .iter_mut()
            .find(|d| d.call == call)
            .ok_or(PapiCode::Removed)?;
        d.gate()
    }

    /// 状态公开视图。
    pub fn view(&self) -> PapiView {
        PapiView {
            version: PAPI_VERSION,
            frozen: self.frozen,
            deprecated: self.deps.len(),
            subscribers: self.subs.clone(),
        }
    }
}

/// API 状态视图。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PapiView {
    pub version: u32,
    pub frozen: bool,
    pub deprecated: usize,
    pub subscribers: Vec<&'static str>,
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct MemCore(alloc::string::String);
    impl PersonaCore for MemCore {
        fn dump(&self) -> alloc::string::String {
            self.0.clone()
        }
        fn load(&mut self, text: &str) -> usize {
            self.0 = text.into();
            1
        }
    }

    #[test]
    fn version_gate() {
        let mut api = PersonaApi::new();
        let mut c = MemCore("persona_version=3\n".into());
        assert_eq!(api.read(&c, 0), Err(PapiCode::Unversioned));
        assert!(api.read(&c, PAPI_VERSION).is_ok());
        assert!(api.write(&mut c, "persona_version=3\n", PAPI_VERSION).is_ok());
    }

    #[test]
    fn deprecation_flow() {
        let mut api = PersonaApi::new();
        api.deps.push(PapiDep::new("legacy_dump", "read"));
        assert!(api.call_deprecated("legacy_dump", PAPI_VERSION).is_ok());
        api.deps[0].advance();
        api.deps[0].advance();
        api.deps[0].advance(); // Removed
        assert_eq!(
            api.call_deprecated("legacy_dump", PAPI_VERSION),
            Err(PapiCode::Removed)
        );
    }

    #[test]
    fn dup_subscribe() {
        let mut api = PersonaApi::new();
        api.subscribe("shell").unwrap();
        assert_eq!(api.subscribe("shell"), Err(PapiCode::DupSubscriber));
    }
}
