//! VE-F3427 · 部分换肤（组件级隔离）—— 隔离器 + 边界断言 + 级联声明 + 局部回滚。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3427`
//!
//! # 职责（锚点原文拆解）
//!
//! - **组件级隔离**：只换一个面板的皮肤，其余部分一动不动。隔离靠
//!   [`Isolation`] 划定边界——边界内组件用新令牌，边界外仍读旧令牌。
//!   边界是**组件集合**，不是命名空间：同一个 `color.accent` 在边界内
//!   是新值、边界外是旧值，令牌路径本身不变；
//! - **边界断言**：换完之后必须实测「边界外零变化」
//!   （[`BoundaryAssertion`]）。断言不是「我们觉得没变」，是逐个组件
//!   比对渲染指纹：边界外任一组件指纹变化即 [`PartialCode::BoundaryLeak`]，
//!   整次换肤回滚到局部快照；
//! - **级联影响显式声明**：有些变化是**故意的**跨界——比如换了主按钮
//!   颜色，同色的强调文字也要跟着变。这类跨界必须逐条声明
//!   （[`CascadeDecl`]），声明里写清「哪个组件、为什么」。没声明的跨界
//!   一律驳回：把「意外」和「设计」分开，是隔离能不能长期维护的关键；
//! - **局部回滚**：隔离失效时只回滚边界内的组件，边界外不参与回滚
//!   （它们本来就没动）。局部回滚比整体回滚快一个量级，也不会让
//!   无关区域的动画被打断。
//!
//! # 为什么以组件集合为边界而不是命名空间
//!
//! 命名空间边界很好实现（换一批令牌），但它答不上「同一令牌在两处
//! 要有不同值」这个真实需求——设置面板想试新皮肤，主界面不变，两者
//! 都读 `color.bg`。以组件集合为边界，令牌解析变成「组件 → 所在隔离
//! 域 → 令牌表」三跳，代价是一次查表，换来的是真正的局部换肤。
//!
//! # 复杂度
//!
//! - `apply`：O(边界内组件数)；
//! - `assert_boundary`：O(全体组件数)；
//! - `rollback_local`：O(边界内组件数)；
//! - `declare_cascade`：O(1) 摊还。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`。测试区除外。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5458 起）
// ---------------------------------------------------------------------------

/// 部分换肤诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartialCode {
    /// 隔离域为空。
    EmptyScope = 5458,
    /// 隔离域与已存在的域重叠。
    ScopeOverlap = 5459,
    /// 边界泄漏（边界外组件被改）。
    BoundaryLeak = 5460,
    /// 级联未声明。
    UndeclaredCascade = 5461,
    /// 级联声明指向边界内组件（本来就该变，不用声明）。
    CascadeRedundant = 5462,
    /// 级联声明缺理由。
    CascadeNoReason = 5463,
    /// 局部回滚目标不存在。
    NothingToRollback = 5464,
    /// 组件重复出现在隔离域。
    DuplicateComponent = 5465,
    /// 隔离域外的令牌被写（越界写）。
    OutOfScopeWrite = 5466,
}

impl PartialCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PartialCode::EmptyScope => "隔离域为空",
            PartialCode::ScopeOverlap => "隔离域与已有域重叠",
            PartialCode::BoundaryLeak => "边界泄漏",
            PartialCode::UndeclaredCascade => "级联影响未声明",
            PartialCode::CascadeRedundant => "级联声明冗余",
            PartialCode::CascadeNoReason => "级联声明缺理由",
            PartialCode::NothingToRollback => "局部回滚目标不存在",
            PartialCode::DuplicateComponent => "组件在隔离域内重复",
            PartialCode::OutOfScopeWrite => "越界写令牌",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, PartialCode::BoundaryLeak | PartialCode::OutOfScopeWrite)
    }
}

// ---------------------------------------------------------------------------
// 二、隔离域
// ---------------------------------------------------------------------------

/// 隔离域：一组组件 + 它们使用的一套令牌。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Isolation {
    /// 域 id。
    pub id: String,
    /// 域内组件。
    pub components: Vec<u32>,
    /// 域内令牌表（路径 → 值）。
    pub tokens: Vec<(String, String)>,
}

impl Isolation {
    /// 构造并校验（非空、无重复）。
    pub fn new(id: &str, components: Vec<u32>) -> Result<Isolation, PartialCode> {
        if components.is_empty() {
            return Err(PartialCode::EmptyScope);
        }
        let mut sorted = components.clone();
        sorted.sort_unstable();
        let mut i = 0;
        while i + 1 < sorted.len() {
            if sorted.get(i) == sorted.get(i + 1) {
                return Err(PartialCode::DuplicateComponent);
            }
            i += 1;
        }
        Ok(Isolation {
            id: id.to_string(),
            components: sorted,
            tokens: Vec::new(),
        })
    }

    /// 域内写令牌。只允许写域内组件读的路径——这里用「域内令牌表」
    /// 隐式约束：不在表里的路径先登记再写。
    pub fn set(&mut self, path: &str, value: &str) {
        if let Some(slot) = self.tokens.iter_mut().find(|(p, _)| p == path) {
            slot.1 = value.to_string();
            return;
        }
        self.tokens.push((path.to_string(), value.to_string()));
    }

    /// 组件是否在域内。
    pub fn contains(&self, component: u32) -> bool {
        self.components.iter().any(|c| *c == component)
    }

    /// 是否与另一个域重叠。
    pub fn overlaps(&self, other: &Isolation) -> bool {
        self.components
            .iter()
            .any(|c| other.components.iter().any(|o| o == c))
    }
}

/// 级联声明：边界外的组件「故意」跟着变。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CascadeDecl {
    /// 边界外组件。
    pub component: u32,
    /// 受影响的路径。
    pub path: String,
    /// 为什么必须跟着变。
    pub reason: String,
}

impl CascadeDecl {
    /// 构造。
    pub fn new(component: u32, path: &str, reason: &str) -> CascadeDecl {
        CascadeDecl {
            component,
            path: path.to_string(),
            reason: reason.to_string(),
        }
    }
}

/// 组件渲染指纹（换肤前后各取一次）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RenderPrint {
    /// 组件。
    pub component: u32,
    /// 指纹。
    pub hash: u64,
}

/// 边界断言结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BoundaryAssertion {
    /// 边界外被改动的组件。
    pub leaks: Vec<u32>,
    /// 检查组件数。
    pub checked: u32,
}

impl BoundaryAssertion {
    /// 是否守住边界。
    pub fn clean(&self) -> bool {
        self.leaks.is_empty()
    }

    /// 读屏渲染。
    pub fn render(&self) -> String {
        if self.clean() {
            format!("边界断言通过：{} 个边界外组件零变化", self.checked)
        } else {
            format!(
                "边界断言失败：{} 个组件越界变化（{}）",
                self.leaks.len(),
                self.leaks
                    .iter()
                    .map(|c| format!("{}", c))
                    .collect::<Vec<_>>()
                    .join("、")
            )
        }
    }
}

/// 边界断言：比对换肤前后的组件指纹。
/// `scope` 内组件允许变；`declared` 里的组件允许变（已声明级联）；
/// 其余任何变化都是泄漏。
pub fn assert_boundary(
    before: &[RenderPrint],
    after: &[RenderPrint],
    scope: &Isolation,
    declared: &[CascadeDecl],
) -> BoundaryAssertion {
    let mut leaks: Vec<u32> = Vec::new();
    let mut checked = 0u32;
    for old in before.iter() {
        if scope.contains(old.component) {
            continue;
        }
        checked = checked.saturating_add(1);
        let new = after.iter().find(|p| p.component == old.component);
        match new {
            Some(n) => {
                if n.hash != old.hash
                    && !declared.iter().any(|d| d.component == old.component)
                {
                    leaks.push(old.component);
                }
            }
            None => {
                // 组件消失了：对边界外来说这也是变化。
                if !declared.iter().any(|d| d.component == old.component) {
                    leaks.push(old.component);
                }
            }
        }
    }
    leaks.sort_unstable();
    BoundaryAssertion { leaks, checked }
}

// ---------------------------------------------------------------------------
// 三、部分换肤执行器
// ---------------------------------------------------------------------------

/// 局部快照：只存边界内的旧令牌。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LocalSnapshot {
    /// 域 id。
    pub scope_id: String,
    /// 旧令牌。
    pub tokens: Vec<(String, String)>,
}

/// 执行结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApplyOutcome {
    /// 是否成功。
    pub applied: bool,
    /// 边界断言。
    pub boundary: BoundaryAssertion,
    /// 是否触发了局部回滚。
    pub rolled_back: bool,
}

/// 部分换肤执行器。
#[derive(Clone, Debug)]
pub struct PartialReskin {
    scope: Isolation,
    declared: Vec<CascadeDecl>,
    snapshot: Option<LocalSnapshot>,
    applied: bool,
}

impl PartialReskin {
    /// 新建。
    pub fn new(scope: Isolation) -> PartialReskin {
        PartialReskin {
            scope,
            declared: Vec::new(),
            snapshot: None,
            applied: false,
        }
    }

    /// 声明一条级联。三条规则：组件必须在边界外（否则冗余）、路径非空、
    /// 理由非空。
    pub fn declare_cascade(&mut self, decl: CascadeDecl) -> Result<(), PartialCode> {
        if self.scope.contains(decl.component) {
            return Err(PartialCode::CascadeRedundant);
        }
        if decl.reason.trim().is_empty() {
            return Err(PartialCode::CascadeNoReason);
        }
        if decl.path.trim().is_empty() {
            return Err(PartialCode::CascadeNoReason);
        }
        if self.declared.iter().any(|d| {
            d.component == decl.component && d.path == decl.path
        }) {
            return Err(PartialCode::UndeclaredCascade);
        }
        self.declared.push(decl);
        Ok(())
    }

    /// 应用：先存局部快照，再写域内令牌。
    pub fn apply(&mut self, new_tokens: &[(&str, &str)]) -> LocalSnapshot {
        let snapshot = LocalSnapshot {
            scope_id: self.scope.id.clone(),
            tokens: self.scope.tokens.clone(),
        };
        for (path, value) in new_tokens.iter() {
            self.scope.set(path, value);
        }
        self.snapshot = Some(snapshot.clone());
        self.applied = true;
        snapshot
    }

    /// 校验边界并按需局部回滚。
    pub fn verify(
        &mut self,
        before: &[RenderPrint],
        after: &[RenderPrint],
    ) -> ApplyOutcome {
        let boundary = assert_boundary(before, after, &self.scope, &self.declared);
        if boundary.clean() {
            return ApplyOutcome {
                applied: true,
                boundary,
                rolled_back: false,
            };
        }
        let rolled = self.rollback_local().is_ok();
        ApplyOutcome {
            applied: false,
            boundary,
            rolled_back: rolled,
        }
    }

    /// 局部回滚：把域内令牌恢复成快照。
    pub fn rollback_local(&mut self) -> Result<(), PartialCode> {
        let snapshot = match self.snapshot.as_ref() {
            Some(s) => s.clone(),
            None => return Err(PartialCode::NothingToRollback),
        };
        self.scope.tokens = snapshot.tokens;
        self.applied = false;
        Ok(())
    }

    /// 域引用（只读）。
    pub fn scope(&self) -> &Isolation {
        &self.scope
    }

    /// 是否已应用。
    pub fn applied(&self) -> bool {
        self.applied
    }

    /// 声明数。
    pub fn cascade_count(&self) -> usize {
        self.declared.len()
    }

    /// 读屏播报。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "部分换肤：域 {}，{} 个组件，{} 条级联声明，{}",
            self.scope.id,
            self.scope.components.len(),
            self.declared.len(),
            if self.applied { "已应用" } else { "未应用" }
        ));
        for t in self.scope.tokens.iter() {
            out.push(format!("域内令牌 {} = {}", t.0, t.1));
        }
        for d in self.declared.iter() {
            out.push(format!(
                "级联：组件{} 路径{} 理由{}",
                d.component, d.path, d.reason
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn prints(bg: u64, fg: u64) -> (Vec<RenderPrint>, Vec<RenderPrint>) {
        let before = alloc::vec![
            RenderPrint { component: 1, hash: 100 },
            RenderPrint { component: 2, hash: 200 },
            RenderPrint { component: 3, hash: 300 },
        ];
        let after = alloc::vec![
            RenderPrint { component: 1, hash: bg },
            RenderPrint { component: 2, hash: fg },
            RenderPrint { component: 3, hash: 300 },
        ];
        (before, after)
    }

    #[test]
    fn isolation_only_changes_scoped_components() {
        let scope = Isolation::new("settings-panel", alloc::vec![1, 2]).expect("域");
        let mut r = PartialReskin::new(scope);
        r.apply(&[("color.bg", "#101828")]);
        let (before, after) = prints(101, 201);
        let out = r.verify(&before, &after);
        assert!(out.applied);
        assert!(out.boundary.clean());
        assert_eq!(out.boundary.checked, 1, "只检查边界外的组件 3");
        assert!(!out.rolled_back);
    }

    #[test]
    fn leak_outside_boundary_rolls_back_locally() {
        let scope = Isolation::new("panel", alloc::vec![1]).expect("域");
        let mut r = PartialReskin::new(scope);
        r.apply(&[("color.bg", "#101828")]);
        let (before, after) = prints(101, 999);
        let out = r.verify(&before, &after);
        assert!(!out.applied);
        assert_eq!(out.boundary.leaks, alloc::vec![2]);
        assert!(out.rolled_back);
        // 回滚后域内令牌回到旧值（原来为空）。
        assert!(r.scope().tokens.is_empty());
    }

    #[test]
    fn declared_cascade_is_allowed_and_undeclared_is_not() {
        let scope = Isolation::new("panel", alloc::vec![1]).expect("域");
        let mut r = PartialReskin::new(scope);
        r.declare_cascade(CascadeDecl::new(2, "color.accent", "强调文字跟随主按钮"))
            .expect("声明");
        r.apply(&[("color.accent", "#4C8DFF")]);
        let (before, after) = prints(100, 555);
        let out = r.verify(&before, &after);
        assert!(out.applied, "已声明级联不算泄漏");
        assert!(out.boundary.clean());
    }

    #[test]
    fn cascade_inside_scope_is_redundant() {
        let scope = Isolation::new("panel", alloc::vec![1, 2]).expect("域");
        let mut r = PartialReskin::new(scope);
        assert_eq!(
            r.declare_cascade(CascadeDecl::new(2, "color.accent", "没必要"))
                .err(),
            Some(PartialCode::CascadeRedundant)
        );
    }

    #[test]
    fn cascade_without_reason_is_rejected() {
        let scope = Isolation::new("panel", alloc::vec![1]).expect("域");
        let mut r = PartialReskin::new(scope);
        assert_eq!(
            r.declare_cascade(CascadeDecl::new(2, "color.accent", "  "))
                .err(),
            Some(PartialCode::CascadeNoReason)
        );
    }

    #[test]
    fn duplicate_component_in_scope_is_rejected() {
        assert_eq!(
            Isolation::new("x", alloc::vec![1, 1]).err(),
            Some(PartialCode::DuplicateComponent)
        );
        assert_eq!(
            Isolation::new("x", alloc::vec![]).err(),
            Some(PartialCode::EmptyScope)
        );
    }

    #[test]
    fn overlap_detection_is_symmetric() {
        let a = Isolation::new("a", alloc::vec![1, 2]).expect("域");
        let b = Isolation::new("b", alloc::vec![2, 3]).expect("域");
        let c = Isolation::new("c", alloc::vec![4, 5]).expect("域");
        assert!(a.overlaps(&b) && b.overlaps(&a));
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn reading_lines_show_scope_cascades_and_tokens() {
        let scope = Isolation::new("panel", alloc::vec![1, 2]).expect("域");
        let mut r = PartialReskin::new(scope);
        r.declare_cascade(CascadeDecl::new(3, "color.accent", "强调色跟随"));
        r.apply(&[("color.accent", "#4C8DFF")]);
        let lines = r.reading_lines();
        assert!(lines.first().map(|l| l.contains("2 个组件")).unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("域内令牌 color.accent")));
        assert!(lines.iter().any(|l| l.contains("级联：组件3")));
    }
}
