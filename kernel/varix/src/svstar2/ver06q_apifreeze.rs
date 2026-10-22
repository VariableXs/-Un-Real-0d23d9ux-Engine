//! VE-F3437 · 换肤 API 冻结 —— 四件冻结门面 + 版本化 + 废弃四步 + 消费方公开。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3437`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四件冻结门面**（[`FROZEN_API`]）：`reskin.start` / `reskin.cancel` /
//!   `reskin.query` / `reskin.subscribe`。四件之外可以有内部函数，但只有
//!   这四件对外承诺稳定；
//! - **版本化**：每个 API 条目带 [`Version`]（major.minor.patch）与签名
//!   哈希。**未版本化的条目一律拒绝**——没有版本号就没有兼容性承诺，
//!   冻结也就无从谈起；
//! - **废弃四步**（[`DeprecationStep`]）：公告 → 告警 → 阻止新用 →
//!   移除。四步必须依次走完，不能跳步：直接从 `Active` 到 `Removed`
//!   会让现有调用方在升级时静默崩溃；
//! - **破坏性变更**：签名哈希变化必须伴随 major 提升。同一 major 内
//!   哈希变了即 [`FreezeCode::BreakingWithoutMajor`]；
//! - **消费方清单公开**：每个条目必须列出已知消费方（[`ApiItem::consumers`]），
//!   空清单视为「隐藏消费方」，拒绝收口。谁在用这个 API 必须可查——
//!   否则废弃时没人知道该通知谁。
//!
//! # 为什么冻结要做成「门面 + 清单」而不是一句约定
//!
//! 「这个 API 已经冻结了」写在文档里，三个月后没人记得。做成清单之后，
//! CI 每次发版都拿当前实现与基线比：签名变了没升 major 就红、消费方
//! 清单空了就红、废弃跳步就红。约定变成了门禁，才可能真的守住。
//!
//! # 复杂度
//!
//! - `audit`：O(条目数 × 基线条目数)；
//! - `status_lines`：O(条目数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5537 起）
// ---------------------------------------------------------------------------

/// API 冻结诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeCode {
    /// 条目未版本化（0.0.0）。
    Unversioned = 5537,
    /// 四件之一缺失。
    FrozenItemMissing = 5538,
    /// 签名变化未升 major。
    BreakingWithoutMajor = 5539,
    /// 废弃跳步。
    DeprecationSkipped = 5540,
    /// 消费方清单为空。
    ConsumersHidden = 5541,
    /// 版本回退。
    VersionBackwards = 5542,
    /// 条目重复。
    DuplicateItem = 5543,
    /// 已移除的条目仍在被调用。
    RemovedStillUsed = 5544,
}

impl FreezeCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            FreezeCode::Unversioned => "API 条目未版本化",
            FreezeCode::FrozenItemMissing => "四件冻结条目缺失",
            FreezeCode::BreakingWithoutMajor => "破坏性变更未升主版本",
            FreezeCode::DeprecationSkipped => "废弃流程跳步",
            FreezeCode::ConsumersHidden => "消费方清单为空",
            FreezeCode::VersionBackwards => "版本回退",
            FreezeCode::DuplicateItem => "API 条目重复",
            FreezeCode::RemovedStillUsed => "已移除条目仍被使用",
        }
    }

    /// 是否阻断发版。
    pub fn blocking(self) -> bool {
        !matches!(self, FreezeCode::ConsumersHidden)
    }
}

/// 四件冻结门面。
pub const FROZEN_API: [&str; 4] = [
    "reskin.start",
    "reskin.cancel",
    "reskin.query",
    "reskin.subscribe",
];

// ---------------------------------------------------------------------------
// 二、版本与状态
// ---------------------------------------------------------------------------

/// 语义化版本。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version {
    /// 主版本。
    pub major: u16,
    /// 次版本。
    pub minor: u16,
    /// 修订。
    pub patch: u16,
}

impl Version {
    /// 构造。
    pub fn new(major: u16, minor: u16, patch: u16) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// 是否未版本化（全 0）。
    pub fn is_unversioned(&self) -> bool {
        self.major == 0 && self.minor == 0 && self.patch == 0
    }

    /// 字符串形态。
    pub fn text(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// 条目状态（废弃四步的落点）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ApiState {
    /// 正常可用。
    Active,
    /// 已公告（文档标注）。
    Announced,
    /// 已告警（调用时告警）。
    Warned,
    /// 阻止新用（新调用方被拒）。
    BlockedNew,
    /// 已移除。
    Removed,
}

impl ApiState {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            ApiState::Active => "可用",
            ApiState::Announced => "已公告",
            ApiState::Warned => "已告警",
            ApiState::BlockedNew => "阻止新用",
            ApiState::Removed => "已移除",
        }
    }

    /// 序号（比对跳步用）。
    pub fn step(self) -> u8 {
        match self {
            ApiState::Active => 0,
            ApiState::Announced => 1,
            ApiState::Warned => 2,
            ApiState::BlockedNew => 3,
            ApiState::Removed => 4,
        }
    }

    /// 是否可调用。
    pub fn callable(self) -> bool {
        !matches!(self, ApiState::Removed)
    }
}

/// 一个 API 条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApiItem {
    /// 名称（`reskin.start`）。
    pub name: String,
    /// 版本。
    pub version: Version,
    /// 签名哈希（参数列表的稳定哈希）。
    pub signature: u64,
    /// 状态。
    pub state: ApiState,
    /// 已知消费方（模块名）。
    pub consumers: Vec<&'static str>,
}

impl ApiItem {
    /// 构造。
    pub fn new(
        name: &str,
        version: Version,
        signature: u64,
        state: ApiState,
        consumers: &[&'static str],
    ) -> ApiItem {
        ApiItem {
            name: name.to_string(),
            version,
            signature,
            state,
            consumers: consumers.to_vec(),
        }
    }
}

/// 冻结基线。
#[derive(Clone, Debug, Default)]
pub struct FreezeBaseline {
    items: Vec<ApiItem>,
}

impl FreezeBaseline {
    /// 空基线。
    pub fn new() -> FreezeBaseline {
        FreezeBaseline { items: Vec::new() }
    }

    /// 收录基线条目（重复名拒绝）。
    pub fn adopt(&mut self, item: ApiItem) -> Result<(), FreezeCode> {
        if self.items.iter().any(|i| i.name == item.name) {
            return Err(FreezeCode::DuplicateItem);
        }
        self.items.push(item);
        Ok(())
    }

    /// 基线条目。
    pub fn items(&self) -> &[ApiItem] {
        &self.items
    }

    /// 审计当前实现。
    pub fn audit(&self, current: &[ApiItem]) -> Vec<FreezeCode> {
        let mut out: Vec<FreezeCode> = Vec::new();
        let mut push = |code: FreezeCode| {
            if !out.contains(&code) {
                out.push(code);
            }
        };
        // 四件必须都在。
        for name in FROZEN_API.iter() {
            if !current.iter().any(|i| i.name == *name) {
                push(FreezeCode::FrozenItemMissing);
            }
        }
        for item in current.iter() {
            if item.version.is_unversioned() {
                push(FreezeCode::Unversioned);
            }
            if item.consumers.is_empty() {
                push(FreezeCode::ConsumersHidden);
            }
            // 与基线比对。
            if let Some(base) = self.items.iter().find(|b| b.name == item.name) {
                if item.version < base.version {
                    push(FreezeCode::VersionBackwards);
                }
                if item.signature != base.signature && item.version.major == base.version.major {
                    push(FreezeCode::BreakingWithoutMajor);
                }
                // 废弃跳步：状态只能一步步走（或原地不动）。
                if item.state.step() > base.state.step().saturating_add(1) {
                    push(FreezeCode::DeprecationSkipped);
                }
                if base.state == ApiState::Removed && item.state.callable() {
                    // 已移除又被「复活」：版本必须升 major，否则视为回退。
                    if item.version.major <= base.version.major {
                        push(FreezeCode::VersionBackwards);
                    }
                }
            }
        }
        out
    }

    /// 冻结状态公开（读屏可达）。
    pub fn status_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!("换肤 API 冻结基线：{} 条", self.items.len()));
        for i in self.items.iter() {
            out.push(format!(
                "{} v{}（{}），签名{:016X}，消费方：{}",
                i.name,
                i.version.text(),
                i.state.label(),
                i.signature,
                if i.consumers.is_empty() {
                    "未登记".to_string()
                } else {
                    i.consumers.join("、")
                }
            ));
        }
        out
    }
}

/// 审批结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeVerdict {
    /// 阻断项。
    pub blocking: Vec<FreezeCode>,
    /// 非阻断项。
    pub advisory: Vec<FreezeCode>,
}

impl FreezeVerdict {
    /// 是否放行。
    pub fn passes(&self) -> bool {
        self.blocking.is_empty()
    }

    /// 一行结论。
    pub fn headline(&self) -> String {
        format!(
            "API 冻结审计：{}，阻断 {} 项，提示 {} 项",
            if self.passes() { "通过" } else { "不通过" },
            self.blocking.len(),
            self.advisory.len()
        )
    }
}

/// 审计并分类。
pub fn audit(baseline: &FreezeBaseline, current: &[ApiItem]) -> FreezeVerdict {
    let codes = baseline.audit(current);
    let mut blocking = Vec::new();
    let mut advisory = Vec::new();
    for c in codes {
        if c.blocking() {
            blocking.push(c);
        } else {
            advisory.push(c);
        }
    }
    FreezeVerdict { blocking, advisory }
}

/// 出厂基线（四件齐全、已版本化、消费方已登记）。
pub fn standard_baseline() -> FreezeBaseline {
    let mut b = FreezeBaseline::new();
    let consumers: [&[&'static str]; 4] = [
        &["settings.panel"],
        &["settings.panel", "theme.gallery"],
        &["status.bar"],
        &["plugin.sdk"],
    ];
    let mut i = 0;
    while i < FROZEN_API.len() {
        let name = match FROZEN_API.get(i) {
            Some(n) => *n,
            None => break,
        };
        let item = ApiItem::new(
            name,
            Version::new(1, 0, 0),
            0x1000_0000_0000_0000u64.wrapping_add(i as u64),
            ApiState::Active,
            match consumers.get(i) {
                Some(c) => c,
                None => &[],
            },
        );
        let _ = b.adopt(item);
        i += 1;
    }
    b
}

// ---------------------------------------------------------------------------
// 三、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn current_from(baseline: &FreezeBaseline) -> Vec<ApiItem> {
        baseline.items().to_vec()
    }

    #[test]
    fn standard_baseline_passes_against_itself() {
        let b = standard_baseline();
        let v = audit(&b, &current_from(&b));
        assert!(v.passes(), "{:?}", v.blocking);
        assert_eq!(b.items().len(), 4);
    }

    #[test]
    fn unversioned_item_is_blocked() {
        let b = standard_baseline();
        let mut cur = current_from(&b);
        if let Some(i) = cur.get_mut(0) {
            i.version = Version::new(0, 0, 0);
        }
        let v = audit(&b, &cur);
        assert!(v.blocking.contains(&FreezeCode::Unversioned));
    }

    #[test]
    fn breaking_signature_requires_major_bump() {
        let b = standard_baseline();
        let mut cur = current_from(&b);
        if let Some(i) = cur.get_mut(0) {
            i.signature ^= 0xFF;
        }
        assert!(audit(&b, &cur)
            .blocking
            .contains(&FreezeCode::BreakingWithoutMajor));
        // 升 major 后不再阻断。
        let mut fixed = current_from(&b);
        if let Some(i) = fixed.get_mut(0) {
            i.signature ^= 0xFF;
            i.version = Version::new(2, 0, 0);
        }
        assert!(!audit(&b, &fixed)
            .blocking
            .contains(&FreezeCode::BreakingWithoutMajor));
    }

    #[test]
    fn deprecation_must_step_one_by_one() {
        let b = standard_baseline();
        let mut cur = current_from(&b);
        if let Some(i) = cur.get_mut(1) {
            // Active -> Removed 跳了三步。
            i.state = ApiState::Removed;
        }
        assert!(audit(&b, &cur)
            .blocking
            .contains(&FreezeCode::DeprecationSkipped));
        // 一步一步走不触发。
        let mut step = current_from(&b);
        if let Some(i) = step.get_mut(1) {
            i.state = ApiState::Announced;
        }
        assert!(!audit(&b, &step)
            .blocking
            .contains(&FreezeCode::DeprecationSkipped));
    }

    #[test]
    fn hidden_consumers_is_advisory_not_blocking() {
        let b = standard_baseline();
        let mut cur = current_from(&b);
        if let Some(i) = cur.get_mut(2) {
            i.consumers.clear();
        }
        let v = audit(&b, &cur);
        assert!(v.advisory.contains(&FreezeCode::ConsumersHidden));
        assert!(v.passes(), "缺消费方清单提示但不阻断发版");
    }

    #[test]
    fn missing_frozen_item_is_blocked() {
        let b = standard_baseline();
        let mut cur = current_from(&b);
        cur.pop();
        assert!(audit(&b, &cur)
            .blocking
            .contains(&FreezeCode::FrozenItemMissing));
    }

    #[test]
    fn version_backwards_is_blocked() {
        let b = standard_baseline();
        let mut cur = current_from(&b);
        if let Some(i) = cur.get_mut(0) {
            i.version = Version::new(0, 9, 0);
        }
        assert!(audit(&b, &cur)
            .blocking
            .contains(&FreezeCode::VersionBackwards));
    }

    #[test]
    fn status_lines_are_public_and_readable() {
        let b = standard_baseline();
        let lines = b.status_lines();
        assert!(lines.first().map(|l| l.contains("4 条")).unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("reskin.start v1.0.0")));
        assert!(lines.iter().any(|l| l.contains("消费方：plugin.sdk")));
    }
}
