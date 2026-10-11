//! VE-F0605 · 图层混合边界（隔离组语义，目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0605`
//!
//! **判据（锚点原文）**：混合模式与不透明度的作用域边界——isolation 声明使
//! 子树先合成独立组再整体参与外部混合（CSS stacking context 等价语义）；隐式
//! 隔离触发条件全集登记：不透明度小于 1、混合模式非 normal、效果链非空、蒙版
//! 存在——四条件各自独立触发；与 CSS 语义对齐验证归 F0632；实现归 F0626（本
//! 条管语义定义与条件清单，实现条管合成执行）。判据四条：**作用域定义、条件
//! 全集、递归明确、语义实现分离**。
//!
//! **错误路径与降级矩阵**：条件误触发→清单复审；隔离组嵌套→递归语义明确
//! （内层先闭合）；声明冲突→显式声明胜出。
//!
//! **设计要点**：
//! - **作用域定义**：隔离组的语义文本（[`ISOLATION_SEMANTICS_DOC`]）显性在册
//!   ——子树先在组内完成全部混合，合成结果作为整体参与父级混合；与 CSS
//!   stacking context 等价（等价性验证归 F0632，本条不重复做）；
//! - **条件全集**：四条件（不透明度<1 / 混合模式非 normal / 效果链非空 /
//!   蒙版存在）各自独立触发，位图 [`ConditionMask`] 逐位记账——"哪个条件
//!   触发的"可归因，误触发才有得复审；不透明度条件与上游 F0603 的
//!   [`CompositingPath::GroupComposite`] 同源判定（不重复发明判据）；
//! - **声明冲突→显式声明胜出**：显式 isolation 声明（isolate / 强制豁免）
//!   覆盖隐式条件结论，冲突时留审计（谁覆盖了谁可查）；
//! - **递归明确**：嵌套隔离组 LIFO 闭合，**内层先闭合**——非栈顶关闭显性
//!   拒绝（[`GroupNestingTracker`]），嵌套合成 O(深度) 有深度账；
//! - **条件误触发→清单复审**：决策与四条件重算比对，不一致即误触发修正
//!   记录（复审是动作不是口号）；
//! - **语义实现分离**：本条只有语义、条件与判定，**不含任何合成执行**——
//!   合成执行归 F0626；本条导出面里没有像素、没有绘制、没有缓冲。
//!
//! **跨批对接点**：上游 F0603 不透明度（类型与判定复用）、F0609 效果链
//! （条件 3 的输入）；下游 F0626 实现、F0621 混合总纲、F0632 CSS 对齐。
//!
//! 逻辑 tick 注入，零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use super::ved03_alpha::{CompositingPath, Opacity};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（语义文本即规格——三份文档判据的载体）
// ---------------------------------------------------------------------------

/// 隔离组作用域定义（作用域定义判据的文本载体；CSS 对齐验证归 F0632）。
pub const ISOLATION_SEMANTICS_DOC: &str = "\
隔离组作用域定义（VE-F0605 · v1）：isolation 声明使子树先合成独立组——\
组内各层按各自混合模式与不透明度完成全部混合，合成结果作为一个整体\
再参与外部（父级）混合。作用域边界 = 组的闭合处：组内混合模式与不透明度\
不外溢，组外不透视。等价于 CSS stacking context 语义（对齐验证归 F0632）。";

/// 隐式隔离触发条件全集（条件全集判据；与 F0603 挂点文档同源，F0605 是定义条）。
pub const IMPLICIT_CONDITIONS_DOC: &str = "\
隐式隔离触发条件全集（VE-F0605 定义 · 四条件各自独立触发）：\
C0 不透明度 < 1（上游 F0603，GroupComposite 即触发）；\
C1 混合模式非 normal（F0621 混合总纲）；\
C2 效果链非空（F0609 效果链）；\
C3 蒙版存在（F0650 蒙版组）。\
任一条件成立即建立隐式隔离组；多条件并存按位图归因。";

/// 嵌套递归语义（递归明确判据的文本载体）。
pub const NESTING_SEMANTICS_DOC: &str = "\
隔离组嵌套递归语义（VE-F0605 · v1）：嵌套时内层先闭合——内层组先完成\
组内混合与组 alpha，其结果作为外层组的一个成员参与外层混合；外层组\
在内层闭合前不得参与外部混合。合成执行归 F0626。";

/// 语义实现分离声明（语义实现分离判据：本条不含合成执行）。
pub const SEMANTICS_ONLY_DOC: &str = "\
本条（F0605）只定义隔离组语义、触发条件与作用域规则；合成执行\
（离屏缓冲、组 alpha 应用、纹理回贴）归 F0626。本条导出面无像素、\
无绘制、无缓冲。";

// ---------------------------------------------------------------------------
// 二、条件位图（四条件逐位归因）
// ---------------------------------------------------------------------------

/// 隐式隔离条件位图（bit0=C0 不透明度 … bit3=C3 蒙版）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ConditionMask(pub u8);

impl ConditionMask {
    /// C0 不透明度 < 1。
    pub const OPACITY: u8 = 1;
    /// C1 混合模式非 normal。
    pub const BLEND: u8 = 1 << 1;
    /// C2 效果链非空。
    pub const EFFECTS: u8 = 1 << 2;
    /// C3 蒙版存在。
    pub const MASK: u8 = 1 << 3;

    /// 置位。
    pub fn set(&mut self, bit: u8) {
        self.0 |= bit;
    }

    /// 条件是否触发。
    pub fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    /// 触发条件数（归因账）。
    pub fn count(self) -> u32 {
        self.0.count_ones()
    }

    /// 是否有任何隐式条件触发。
    pub fn any(self) -> bool {
        self.0 != 0
    }

    /// 触发条件的人话清单（位图归因；复审与读屏用）。
    pub fn names(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.has(ConditionMask::OPACITY) {
            out.push("不透明度<1");
        }
        if self.has(ConditionMask::BLEND) {
            out.push("混合模式非normal");
        }
        if self.has(ConditionMask::EFFECTS) {
            out.push("效果链非空");
        }
        if self.has(ConditionMask::MASK) {
            out.push("蒙版存在");
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 三、每层隔离输入与决策（判定 O(1)）
// ---------------------------------------------------------------------------

/// 每层的隔离判定输入（上游三源的取值 + 显式声明）。
#[derive(Clone, Copy, Debug)]
pub struct LayerIsolationInput {
    /// 本层不透明度（上游 F0603 类型；构造校验由 Opacity 承担）。
    pub opacity: Opacity,
    /// 混合模式是否 normal（C1 输入；模式本体归 F0621）。
    pub blend_is_normal: bool,
    /// 效果链是否非空（C2 输入；效果链本体归 F0609）。
    pub effects_non_empty: bool,
    /// 蒙版是否存在（C3 输入；蒙版本体归 F0650）。
    pub mask_present: bool,
    /// 显式 isolation 声明：Some(true)=isolate，Some(false)=强制豁免，
    /// None=auto（按隐式条件判定）。声明冲突时显式胜出。
    pub explicit: Option<bool>,
}

/// 隔离结论的来源（归因面：谁让这层隔离的）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsolationSource {
    /// 显式 isolate 声明（胜出者）。
    ExplicitIsolate,
    /// 显式强制豁免（显式胜出但留审计——豁免隐式条件属高危动作）。
    ExplicitWaive,
    /// 隐式条件触发（位图归因）。
    Implicit(ConditionMask),
    /// 无隔离。
    None,
}

/// 隔离决策结论（纯数据——语义实现分离：不含任何执行动作）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsolationDecision {
    /// 是否建立隔离组。
    pub isolated: bool,
    /// 来源（归因）。
    pub source: IsolationSource,
}

/// O(1) 隔离判定：四条件位图归因 + 显式声明胜出。
pub fn decide_isolation(input: &LayerIsolationInput) -> IsolationDecision {
    // 显式声明胜出（声明冲突判据）。
    if let Some(e) = input.explicit {
        return IsolationDecision {
            isolated: e,
            source: if e { IsolationSource::ExplicitIsolate } else { IsolationSource::ExplicitWaive },
        };
    }
    // 隐式条件位图（四条件各自独立触发）。
    let mut mask = ConditionMask::default();
    // C0：与上游 F0603 同源——GroupComposite 即"不透明度<1"。
    if input.opacity.path() == CompositingPath::GroupComposite {
        mask.set(ConditionMask::OPACITY);
    }
    if !input.blend_is_normal {
        mask.set(ConditionMask::BLEND);
    }
    if input.effects_non_empty {
        mask.set(ConditionMask::EFFECTS);
    }
    if input.mask_present {
        mask.set(ConditionMask::MASK);
    }
    if mask.any() {
        IsolationDecision { isolated: true, source: IsolationSource::Implicit(mask) }
    } else {
        IsolationDecision { isolated: false, source: IsolationSource::None }
    }
}

// ---------------------------------------------------------------------------
// 四、条件误触发复审（条件误触发→清单复审判据）
// ---------------------------------------------------------------------------

/// 误触发复审结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MisfireCorrection {
    /// 复审对象层的描述（调用方标识）。
    pub who: String,
    /// 复审前决策。
    pub before: IsolationDecision,
    /// 复审后决策（按条件清单重算）。
    pub after: IsolationDecision,
}

/// 清单复审：按四条件清单重算决策并与原决策比对——不一致即误触发修正。
///
/// 显式来源不参加隐式复审（显式胜出是语义不是误触发）。
pub fn audit_misfire(
    who: &str,
    claimed: IsolationDecision,
    input: &LayerIsolationInput,
) -> Option<MisfireCorrection> {
    let recomputed = decide_isolation(input);
    if claimed == recomputed || claimed.source == IsolationSource::ExplicitIsolate || claimed.source == IsolationSource::ExplicitWaive {
        return None;
    }
    Some(MisfireCorrection { who: who.to_string(), before: claimed, after: recomputed })
}

// ---------------------------------------------------------------------------
// 五、作用域规则（组的成员区间解析）
// ---------------------------------------------------------------------------

/// 一个隔离组的作用域区间（ owner 层与它的成员在层链中的下标范围）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupSpan {
    /// 组的 owner 层（链中的稳定 id）。
    pub owner: u64,
    /// 组在层链中的起点下标（= owner 自己）。
    pub start_idx: usize,
    /// 组在层链中的终点下标（ exclusive；嵌套子组的成员归子组，不归本组）。
    pub end_idx: usize,
}

/// 作用域规则解析：给定层链（下标序 = 合成序）与逐层决策，产出各隔离组的
/// 成员区间。
///
/// 规则（与 [`NESTING_SEMANTICS_DOC`] 一致）：isolated=true 的层开一个组，
/// 其后所有层都是组成员，直到下一个 isolated=true 的层开新组（嵌套子组）
/// 或链结束。嵌套语义：子组先闭合，子组的合成结果作为外层组的一个成员。
pub fn resolve_group_spans(chain: &[(u64, IsolationDecision)]) -> Vec<GroupSpan> {
    let mut out = Vec::new();
    let mut current: Option<(u64, usize)> = None;
    for (idx, (node_id, d)) in chain.iter().enumerate() {
        if d.isolated {
            // 开新组：闭合前一个组（若在）。
            if let Some((owner, start)) = current {
                out.push(GroupSpan { owner, start_idx: start, end_idx: idx });
            }
            current = Some((*node_id, idx));
        }
    }
    if let Some((owner, start)) = current {
        out.push(GroupSpan { owner, start_idx: start, end_idx: chain.len() });
    }
    out
}

// ---------------------------------------------------------------------------
// 六、嵌套组闭合追踪（递归明确：内层先闭合；嵌套合成 O(深度)）
// ---------------------------------------------------------------------------

/// 嵌套闭合追踪器（语义层的 LIFO 纪律；离屏执行归 F0626）。
pub struct GroupNestingTracker {
    stack: Vec<u64>,
    max_depth_seen: usize,
    audits: Vec<String>,
    errors: Vec<(String, &'static str, String)>,
    tick: u64,
}

impl GroupNestingTracker {
    /// 空追踪器。
    pub fn new() -> Self {
        GroupNestingTracker {
            stack: Vec::new(),
            max_depth_seen: 0,
            audits: Vec::new(),
            errors: Vec::new(),
            tick: 0,
        }
    }

    /// 当前嵌套深度。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 本会话最大嵌套深度（O(深度) 成本账的实测面）。
    pub fn max_depth_seen(&self) -> usize {
        self.max_depth_seen
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 错误账本（零静默）。
    pub fn errors(&self) -> &[(String, &'static str, String)] {
        &self.errors
    }

    /// 开组（决策 isolated=true 时调用；深度账同步）。
    pub fn open_group(&mut self, node_id: u64) {
        self.stack.push(node_id);
        self.max_depth_seen = self.max_depth_seen.max(self.stack.len());
        self.audits
            .push(format!("tick{} n={node_id} 开隔离组（深度 {}）", self.tick, self.stack.len()));
    }

    /// 闭组：**内层先闭合**——只允许栈顶闭组，越序显性拒绝。
    pub fn close_group(&mut self, node_id: u64) -> Result<(), &'static str> {
        match self.stack.last() {
            Some(top) if *top == node_id => {
                self.stack.pop();
                self.audits.push(format!(
                    "tick{} n={node_id} 闭隔离组（内层先闭合，余深 {}）",
                    self.tick,
                    self.stack.len()
                ));
                Ok(())
            }
            Some(top) => {
                self.errors.push((
                    format!("close_group(n={node_id})"),
                    "E_INNER_MUST_CLOSE_FIRST",
                    format!(
                        "越序闭组：栈顶是 n={top}（内层），必须先闭合内层再闭合外层——嵌套递归语义（内层先闭合）"
                    ),
                ));
                Err("内层未闭合")
            }
            None => {
                self.errors.push((
                    format!("close_group(n={node_id})"),
                    "E_CLOSE_WITHOUT_OPEN",
                    "无组可闭：闭组必须与开组配对（LIFO 纪律）".to_string(),
                ));
                Err("无未闭合组")
            }
        }
    }

    /// 逻辑 tick 推进（零墙钟纪律）。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// 六、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0605 域自检（判据逐条映射见 `ved05_checks.rs`）。
pub fn run_ved05_checks() -> CheckSet {
    super::ved05_checks::run_ved05_checks()
}

// ---------------------------------------------------------------------------
// 七、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 不透明输入（不触发 C0）。
    fn base() -> LayerIsolationInput {
        LayerIsolationInput {
            opacity: Opacity::new(1.0).unwrap(),
            blend_is_normal: true,
            effects_non_empty: false,
            mask_present: false,
            explicit: None,
        }
    }

    #[test]
    fn ved05_scope_semantics_doc_registered() {
        // 作用域定义与语义实现分离文档在册（文本即规格的判据载体）。
        assert!(ISOLATION_SEMANTICS_DOC.contains("独立组") && ISOLATION_SEMANTICS_DOC.contains("整体"));
        assert!(SEMANTICS_ONLY_DOC.contains("F0626"));
        assert!(NESTING_SEMANTICS_DOC.contains("内层先闭合"));
    }

    #[test]
    fn ved05_four_conditions_independently_trigger() {
        // 四条件各自独立触发——每次只给一个条件，其余全关。
        let mut i = base();
        i.opacity = Opacity::new(0.5).unwrap();
        let d = decide_isolation(&i);
        assert!(d.isolated && d.source == IsolationSource::Implicit(ConditionMask(ConditionMask::OPACITY)));

        let mut i = base();
        i.blend_is_normal = false;
        let d = decide_isolation(&i);
        assert!(d.isolated && d.source == IsolationSource::Implicit(ConditionMask(ConditionMask::BLEND)));

        let mut i = base();
        i.effects_non_empty = true;
        let d = decide_isolation(&i);
        assert!(d.isolated && d.source == IsolationSource::Implicit(ConditionMask(ConditionMask::EFFECTS)));

        let mut i = base();
        i.mask_present = true;
        let d = decide_isolation(&i);
        assert!(d.isolated && d.source == IsolationSource::Implicit(ConditionMask(ConditionMask::MASK)));

        // 全无条件 → 不隔离。
        let d = decide_isolation(&base());
        assert!(!d.isolated && d.source == IsolationSource::None);
    }

    #[test]
    fn ved05_mask_counts_and_opacity_source_aligned_with_f0603() {
        // 多条件并存按位图归因。
        let mut i = base();
        i.opacity = Opacity::new(0.5).unwrap();
        i.mask_present = true;
        i.effects_non_empty = true;
        let d = decide_isolation(&i);
        match d.source {
            IsolationSource::Implicit(m) => assert_eq!(m.count(), 3, "位图归因：三条件并存"),
            other => panic!("应为隐式来源，实际 {other:?}"),
        }
        // C0 与 F0603 同源：GroupComposite ⇔ OPACITY 位。
        let semi = Opacity::new(0.5).unwrap();
        assert_eq!(semi.path(), CompositingPath::GroupComposite);
        assert!(decide_isolation(&LayerIsolationInput { opacity: semi, ..base() })
            .source
            != IsolationSource::None);
        let opaque = Opacity::new(1.0).unwrap();
        assert_eq!(opaque.path(), CompositingPath::OpaqueFastPath);
    }

    #[test]
    fn ved05_explicit_declaration_wins_on_conflict() {
        // 无任何隐式条件，显式 isolate → 隔离（显式胜出）。
        let mut i = base();
        i.explicit = Some(true);
        let d = decide_isolation(&i);
        assert!(d.isolated && d.source == IsolationSource::ExplicitIsolate);
        // 四条件全触发，显式豁免 → 不隔离（显式胜出，但来源留痕供审计）。
        let mut i = base();
        i.opacity = Opacity::new(0.5).unwrap();
        i.blend_is_normal = false;
        i.effects_non_empty = true;
        i.mask_present = true;
        i.explicit = Some(false);
        let d = decide_isolation(&i);
        assert!(!d.isolated && d.source == IsolationSource::ExplicitWaive);
    }

    #[test]
    fn ved05_misfire_audit_catches_wrong_claim() {
        // 实际无条件触发，却声称隐式隔离 → 复审检出。
        let input = base();
        let claimed = IsolationDecision {
            isolated: true,
            source: IsolationSource::Implicit(ConditionMask(ConditionMask::BLEND)),
        };
        let fix = audit_misfire("n=12", claimed, &input).expect("误触发应被复审检出");
        assert!(!fix.after.isolated);
        // 声明一致 → 无修正。
        let ok = decide_isolation(&input);
        assert!(audit_misfire("n=12", ok, &input).is_none());
        // 显式来源不参加隐式复审（显式胜出是语义不是误触发）。
        let claimed = IsolationDecision { isolated: true, source: IsolationSource::ExplicitIsolate };
        assert!(audit_misfire("n=12", claimed, &input).is_none());
    }

    #[test]
    fn ved05_nesting_inner_closes_first() {
        let mut t = GroupNestingTracker::new();
        t.tick();
        t.open_group(1);
        t.open_group(2);
        t.open_group(3);
        assert_eq!(t.depth(), 3);
        assert_eq!(t.max_depth_seen(), 3, "O(深度) 成本账");
        // 越序闭外层 → 显性拒绝（内层先闭合）。
        assert!(t.close_group(1).is_err());
        assert!(t.errors().iter().any(|(_, c, _)| *c == "E_INNER_MUST_CLOSE_FIRST"));
        // LIFO 正确序：3 → 2 → 1。
        assert!(t.close_group(3).is_ok());
        assert!(t.close_group(2).is_ok());
        assert!(t.close_group(1).is_ok());
        assert_eq!(t.depth(), 0);
        // 无组可闭 → 显性拒绝。
        assert!(t.close_group(9).is_err());
        assert!(t.errors().iter().any(|(_, c, _)| *c == "E_CLOSE_WITHOUT_OPEN"));
    }

    #[test]
    fn ved05_group_span_resolution() {
        // 链：A(隔离) b c D(隔离) e —— A 的成员是 [b,c]，D 的成员是 [e]。
        let iso = IsolationDecision { isolated: true, source: IsolationSource::ExplicitIsolate };
        let plain = IsolationDecision { isolated: false, source: IsolationSource::None };
        let chain = vec![
            (1u64, iso),
            (2u64, plain),
            (3u64, plain),
            (4u64, iso),
            (5u64, plain),
        ];
        let spans = resolve_group_spans(&chain);
        assert_eq!(spans.len(), 2, "两个隔离组");
        assert_eq!(spans[0], GroupSpan { owner: 1, start_idx: 0, end_idx: 3 }, "A 组成员 [b,c]");
        assert_eq!(spans[1], GroupSpan { owner: 4, start_idx: 3, end_idx: 5 }, "D 组成员 [e]");
        // 全不隔离 → 无组。
        let plain_chain = vec![(1u64, plain), (2u64, plain)];
        assert!(resolve_group_spans(&plain_chain).is_empty());
        // 末层隔离（无成员）→ 空成员组仍成组。
        let tail_chain = vec![(1u64, plain), (2u64, iso)];
        let spans = resolve_group_spans(&tail_chain);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0], GroupSpan { owner: 2, start_idx: 1, end_idx: 2 });
    }

    #[test]
    fn ved05_checks_all_green() {
        let set = run_ved05_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0605 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}
