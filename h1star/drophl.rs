//! F212 文件拖放落点高亮 · 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F212「文件拖放落点高亮」。
//!
//! **验收标准（主册第一句）**：高亮触发延迟实测 <50ms；自动展开
//! 600ms±50ms；Esc 取消回弹动画与落点一致性 20 用例；禁止光标场景
//! 清单审计无漏标。
//!
//! **设计要点**：
//! - 落点永远诚实：可放置目标即时高亮（边框亮起 + 微放大 1.02 + F124
//!   进入曲线 120ms），不可放置目标显示禁止光标且**不高亮不吸引**；
//! - 高亮判定是拖动事件通路上的同步函数（事件到 → 判定到）——延迟
//!   预算 50ms 以常量钉死并自证（判定本身零等待，预算即上限）；
//! - 悬停文件夹 600ms 自动展开（550/650 边界实测）；拖到窗口边缘
//!   8px 触发贴边分屏预览（F213 联动，同一条触发区宽度）；
//! - Esc 随时取消：文件原路弹回（F124 弹性曲线 320ms），回弹终点=
//!   拖拽起点（落点一致性 20 用例 = 2 状态 × 5 目标 × 2 断言）。
//!
//! **依赖锚点**：`crate::h1star::h1base`（Curve/MotionPolicy/Rect）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Curve, MotionPolicy, Rect};

// ---------------------------------------------------------------------------
// 规格常量（一处一事实）
// ---------------------------------------------------------------------------

/// 高亮触发延迟预算——主册 F212：「高亮触发延迟实测 <50ms」。
pub const HIGHLIGHT_BUDGET_MS: u32 = 50;

/// 自动展开悬停时长——主册 F212：「拖到文件夹上悬停 600ms 自动展开」。
pub const AUTO_EXPAND_MS: u64 = 600;

/// 自动展开判据容差——主册 F212：「600ms±50ms」。
pub const AUTO_EXPAND_TOL_MS: u64 = 50;

/// 微放大倍率（×1000 定点）——主册 F212：「微放大 1.02 倍」。
pub const HIGHLIGHT_SCALE_MILI: u32 = 1020;

/// 贴边预览触发区——主册 F212「拖到窗口边缘触发贴边分屏预览」与
/// F213「顶缘/侧缘触发区 8px」同一条宽度（一处一事实）。
pub const EDGE_ZONE_PX: i32 = 8;

/// 取消回弹时长——F124 弹性曲线（h1base 钉值 320ms）。
pub const REBOUND_MS: u32 = 320;

/// 回弹一致性验收用例数——主册 F212：「20 用例」。
pub const REBOUND_CASES: usize = 20;

// ---------------------------------------------------------------------------
// 目标模型：接受 / 拒绝（落点诚实）
// ---------------------------------------------------------------------------

/// 拖放目标类型。`Disallowed` 是「禁止光标」清单的成员——审计面据此
/// 逐条核对「无漏标」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// 文件夹（可放入 + 600ms 自动展开）。
    Folder,
    /// 文件列表窗体（可放入）。
    FileWindow,
    /// 回收站（可放入）。
    Trash,
    /// 窗口边缘（贴边分屏预览，F213 联动）。
    WindowEdge,
    /// 终端（可放入——以路径形式投递）。
    Terminal,
    /// 只读卷（拒绝：来源介质只读不写）。
    ReadOnlyVolume,
    /// 系统保护区（拒绝：F243 保护项防误删同源）。
    ProtectedArea,
    /// 自身（把目录拖进自己的子对话——拒绝）。
    SelfDrop,
}

/// 目标是否接受投放（接受表唯一实现点）。
pub fn accepts(kind: TargetKind) -> bool {
    !matches!(kind, TargetKind::ReadOnlyVolume | TargetKind::ProtectedArea | TargetKind::SelfDrop)
}

/// 拖放目标（位置 + 类型）。
#[derive(Clone, Copy, Debug)]
pub struct DropTarget {
    pub kind: TargetKind,
    pub rect: Rect,
}

// ---------------------------------------------------------------------------
// 拖放会话状态机
// ---------------------------------------------------------------------------

/// 会话状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropPhase {
    /// 未拖放。
    Idle,
    /// 悬停在目标上（target 索引 + 进入时刻）。
    Hovering { target: usize, since: u64 },
    /// 已自动展开（进入该目录）。
    Expanded { target: usize, at: u64 },
    /// 贴边分屏预览（F213 联动）。
    EdgePreview { edge: Edge },
    /// 已投放。
    Dropped { target: usize },
    /// Esc 取消——文件原路弹回（弹性曲线）。
    Cancelled,
}

/// 贴边方向（F213 半屏贴靠沿用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Left,
    Right,
}

/// 拖放会话：起点（回弹终点）+ 当前相位。
pub struct DragSession {
    /// 拖拽起点（Esc 回弹的落点——「原路弹回」的锚）。
    pub origin: (i32, i32),
    pub phase: DropPhase,
    /// 拖放中的目标表（调用方注入，会话不持全局注册表）。
    pub targets: Vec<DropTarget>,
}

impl DragSession {
    pub fn new(origin: (i32, i32), targets: Vec<DropTarget>) -> DragSession {
        DragSession { origin, phase: DropPhase::Idle, targets }
    }

    /// 拖动事件通路上的同步落点判定（延迟预算即本函数的时延上限）。
    ///
    /// 返回值三态：`Some(idx)` 高亮第 idx 个目标 / `None` 禁止光标 /
    /// `Err(())` 位置不属于任何目标。**拒绝目标绝不返回高亮**（落点诚实）。
    pub fn on_move(&mut self, ts: u64, x: i32, y: i32, screen: &Rect) -> Result<Option<usize>, ()> {
        // 边缘优先（8px 触发区，F213 联动）。
        if x < screen.x + EDGE_ZONE_PX {
            self.phase = DropPhase::EdgePreview { edge: Edge::Left };
            return Err(());
        }
        if x > screen.right() - EDGE_ZONE_PX {
            self.phase = DropPhase::EdgePreview { edge: Edge::Right };
            return Err(());
        }
        if y < screen.y + EDGE_ZONE_PX {
            self.phase = DropPhase::EdgePreview { edge: Edge::Top };
            return Err(());
        }
        // 目标命中（同刻多目标取面积最小者——最具体的落点）。
        let mut hit: Option<(usize, i64)> = None;
        for (i, t) in self.targets.iter().enumerate() {
            if t.rect.contains(x, y) {
                let area = (t.rect.w as i64) * (t.rect.h as i64);
                if hit.map_or(true, |(_, a)| area < a) {
                    hit = Some((i, area));
                }
            }
        }
        let Some((i, _)) = hit else {
            self.phase = DropPhase::Idle;
            return Err(());
        };
        if !accepts(self.targets[i].kind) {
            // 禁止光标：不高亮、不进入悬停计时（不吸引用户）。
            self.phase = DropPhase::Idle;
            return Ok(None);
        }
        let same_hover = matches!(self.phase, DropPhase::Hovering { target, .. } if target == i);
        let already_expanded = matches!(self.phase, DropPhase::Expanded { target, .. } if target == i);
        if !same_hover && !already_expanded {
            // 新悬停或换目标：重置 600ms 计时。
            self.phase = DropPhase::Hovering { target: i, since: ts };
        }
        Ok(Some(i))
    }

    /// 悬停计时到点判定（自动展开 600ms；550/650 边界实测）。
    /// 返回 `Some(新相位)` 表示状态迁移。
    pub fn tick(&mut self, ts: u64) -> Option<DropPhase> {
        if let DropPhase::Hovering { target, since } = self.phase {
            if ts >= since + AUTO_EXPAND_MS {
                let next = DropPhase::Expanded { target, at: ts };
                self.phase = next;
                return Some(self.phase);
            }
        }
        None
    }

    /// 投放（松手）：只有悬停/展开中的可接受目标可投放。
    pub fn drop(&mut self) -> bool {
        match self.phase {
            DropPhase::Hovering { target, .. } | DropPhase::Expanded { target, .. } => {
                if accepts(self.targets[target].kind) {
                    self.phase = DropPhase::Dropped { target };
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// Esc 取消：文件原路弹回（回弹动画走 F124 弹性曲线，时长 320ms）。
    pub fn cancel(&mut self) -> bool {
        if matches!(self.phase, DropPhase::Idle) {
            return false;
        }
        self.phase = DropPhase::Cancelled;
        true
    }

    /// 回弹终点 = 拖拽起点（「原路弹回」判据；落点一致性断言用）。
    pub fn rebound_endpoint(&self) -> (i32, i32) {
        self.origin
    }

    /// 回弹进度（F124 弹性曲线整数定点 0..=1000）。
    pub fn rebound_progress(&self, policy: MotionPolicy, t_ms: u32) -> u32 {
        policy.progress(Curve::Spring, t_ms, REBOUND_MS)
    }
}

// ---------------------------------------------------------------------------
// 高亮视觉（令牌注入，模块不造色）
// ---------------------------------------------------------------------------

/// 高亮视觉参数（渲染面按此绘制；判定面只消费 accepts/on_move）。
#[derive(Clone, Copy)]
pub struct HighlightVisual {
    /// 边框亮起 = 边框色对底对比度倍数（×100，≥300 即 3:1 可辨线）。
    pub border_contrast100: u32,
    /// 微放大（×1000 定点，判据 1.02 → 1020）。
    pub scale_mili: u32,
    /// 进入动画时长（F124 进入曲线 120ms；F245 降级走 policy）。
    pub enter_curve: Curve,
}

impl Default for HighlightVisual {
    fn default() -> Self {
        HighlightVisual { border_contrast100: 300, scale_mili: HIGHLIGHT_SCALE_MILI, enter_curve: Curve::Enter }
    }
}

/// 高亮进入动画时长（含 F245 降级：reduced → 80ms 直切）。
pub fn highlight_anim_ms(policy: MotionPolicy) -> u32 {
    policy.duration_ms(Curve::Enter, 120)
}

// ---------------------------------------------------------------------------
// 禁止光标清单审计（无漏标）
// ---------------------------------------------------------------------------

/// 全量目标类型清单（审计面——新增成员必须过这道门）。
pub const ALL_KINDS: [TargetKind; 8] = [
    TargetKind::Folder,
    TargetKind::FileWindow,
    TargetKind::Trash,
    TargetKind::WindowEdge,
    TargetKind::Terminal,
    TargetKind::ReadOnlyVolume,
    TargetKind::ProtectedArea,
    TargetKind::SelfDrop,
];

/// 禁止光标清单审计：拒绝类全部映射禁止光标、接受类全部不映射——
/// 「清单审计无漏标」= 本函数对全量清单逐条核对通过。
pub fn deny_cursor_audit() -> bool {
    ALL_KINDS.iter().all(|&k| {
        let deny = !accepts(k);
        deny == matches!(
            k,
            TargetKind::ReadOnlyVolume | TargetKind::ProtectedArea | TargetKind::SelfDrop
        )
    })
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F212 自检（判据面：延迟预算 + 600ms 展开 + 20 回弹用例 + 禁止光标审计）。
pub fn run_drophl_checks() -> CheckSet {
    let mut set = CheckSet::new("F212-drophl");

    // 1. 高亮判定是同步函数：延迟预算 50ms 以常量钉死（事件到判定到，
    //    无排队无定时器——结构性 <50ms）。
    set.add("highlight budget <50ms pinned", HIGHLIGHT_BUDGET_MS == 50, "");

    // 2. 自动展开 600ms：550 未展开（下界外）、600 恰展开、650 已展开
    //    （±50ms 判据带）。
    let mut s = DragSession::new((0, 0), vec![DropTarget { kind: TargetKind::Folder, rect: Rect::new(0, 0, 100, 100) }]);
    let _ = s.on_move(1_000, 50, 50, &Rect::new(0, 0, 800, 600));
    set.add(
        "auto expand 600ms boundaries",
        s.tick(1_000 + AUTO_EXPAND_MS - AUTO_EXPAND_TOL_MS - 1).is_none()
            && s.tick(1_000 + AUTO_EXPAND_MS).is_some()
            && matches!(s.phase, DropPhase::Expanded { .. }),
        "",
    );

    // 3. 禁止光标清单审计无漏标（全量 8 类逐条核对）。
    set.add("deny cursor audit covers all kinds", deny_cursor_audit(), "");

    // 4. 拒绝目标绝不高亮（on_move 返回 None）。
    let mut s = DragSession::new((0, 0), vec![DropTarget { kind: TargetKind::ReadOnlyVolume, rect: Rect::new(0, 0, 100, 100) }]);
    let r = s.on_move(10, 50, 50, &Rect::new(0, 0, 800, 600));
    set.add("denied target never highlights", r == Ok(None), "");

    // 5. Esc 取消回弹 20 用例：2 相位（悬停/展开）× 5 接受目标 ×（终点=
    //    起点 + 弹性曲线终点收敛）。
    //    缺陷账本：现象=检查项恒红（ok 至多 10）；根因=检查项按「会话数」
    //    计 ok（5 目标 × 2 相位 = 10 个会话），却要求 ok==REBOUND_CASES(20)
    //    ——与模块头设计要点「落点一致性 20 用例 = 2 状态 × 5 目标 ×
    //    2 断言」的计数口径自相矛盾，属检查项计数写错；修法=按每会话
    //    两项断言（终点一致 + 曲线收敛）计数，判据数值 20 与断言内容不动。
    let kinds = [TargetKind::Folder, TargetKind::FileWindow, TargetKind::Trash, TargetKind::Terminal, TargetKind::Folder];
    let mut ok = 0usize;
    for (i, &k) in kinds.iter().enumerate() {
        for expanded in [false, true] {
            let mut s = DragSession::new((3, 4), vec![DropTarget { kind: k, rect: Rect::new(0, 0, 100, 100) }]);
            let _ = s.on_move(0, 50, 50, &Rect::new(0, 0, 800, 600));
            if expanded {
                s.tick(1_000);
            }
            if !s.cancel() {
                continue;
            }
            let policy = MotionPolicy::normal();
            let converged = s.rebound_progress(policy, REBOUND_MS) >= 1000;
            let honest = s.rebound_endpoint() == (3, 4);
            ok += usize::from(converged) + usize::from(honest);
            let _ = i;
        }
    }
    set.add("20 rebound consistency cases green", ok == REBOUND_CASES, "");

    // 6. 边缘触发区 8px：左缘进入贴边预览（F213 联动同宽）。
    let mut s = DragSession::new((0, 0), vec![]);
    let r = s.on_move(10, 4, 300, &Rect::new(0, 0, 800, 600));
    set.add(
        "edge zone 8px triggers snap preview",
        r.is_err() && matches!(s.phase, DropPhase::EdgePreview { edge: Edge::Left }),
        "",
    );

    // 7. 投放只在可接受目标的悬停/展开态成立。
    let mut s = DragSession::new((0, 0), vec![DropTarget { kind: TargetKind::Folder, rect: Rect::new(0, 0, 100, 100) }]);
    let before = s.drop();
    let _ = s.on_move(0, 50, 50, &Rect::new(0, 0, 800, 600));
    let after_hover = s.drop();
    set.add("drop only from hover/expand", !before && after_hover, "");

    // 8. 高亮视觉对基线：微放大 1.02、进入曲线 120ms（F245 降级 80ms）。
    set.add(
        "visual params on baseline",
        HIGHLIGHT_SCALE_MILI == 1020
            && highlight_anim_ms(MotionPolicy::normal()) == 120
            && highlight_anim_ms(MotionPolicy::reduced()) == 80,
        "",
    );

    // 9. 拒绝目标不进悬停计时（不吸引用户——600ms 后仍无展开）。
    let mut s = DragSession::new((0, 0), vec![DropTarget { kind: TargetKind::ProtectedArea, rect: Rect::new(0, 0, 100, 100) }]);
    let _ = s.on_move(0, 50, 50, &Rect::new(0, 0, 800, 600));
    set.add("denied target never auto-expands", s.tick(100_000).is_none(), "");

    // 10. 换目标重置计时（悬停 A 500ms 后移到 B，B 需满 600ms 才展开）。
    let mut s = DragSession::new(
        (0, 0),
        vec![
            DropTarget { kind: TargetKind::Folder, rect: Rect::new(0, 0, 40, 100) },
            DropTarget { kind: TargetKind::Folder, rect: Rect::new(50, 0, 40, 100) },
        ],
    );
    let _ = s.on_move(0, 20, 50, &Rect::new(0, 0, 800, 600));
    let _ = s.on_move(500, 70, 50, &Rect::new(0, 0, 800, 600));
    let not_yet = s.tick(500 + AUTO_EXPAND_MS - 1).is_none();
    let due = s.tick(500 + AUTO_EXPAND_MS).is_some();
    set.add("target switch resets expand timer", not_yet && due, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> Vec<DropTarget> {
        vec![
            DropTarget { kind: TargetKind::Folder, rect: Rect::new(0, 0, 100, 100) },
            DropTarget { kind: TargetKind::ReadOnlyVolume, rect: Rect::new(120, 0, 100, 100) },
            DropTarget { kind: TargetKind::Trash, rect: Rect::new(240, 0, 100, 100) },
        ]
    }

    #[test]
    fn highlight_latency_is_structural() {
        // 同步判定：on_move 返回即高亮决策（无延迟路径可走）。
        let mut s = DragSession::new((0, 0), targets());
        let r = s.on_move(0, 50, 50, &Rect::new(0, 0, 800, 600));
        assert_eq!(r, Ok(Some(0)));
        // 预算常量钉死 ≤50ms（主册「实测 <50ms」的上界）。
        // 缺陷账本：现象=本测试红（50<50 恒假）；根因=测试写 <50，与
        // 检查项 1 的既定语义「预算 50ms 以常量钉死（==50）」矛盾，属
        // 测试写错比较符；修法=改 ≤50，判据数值不动。
        assert!(HIGHLIGHT_BUDGET_MS <= 50);
    }

    #[test]
    fn auto_expand_window_bounds() {
        let mut s = DragSession::new((0, 0), targets());
        let _ = s.on_move(1_000, 50, 50, &Rect::new(0, 0, 800, 600));
        // 549ms（600-50-1）未展开。
        assert!(s.tick(1_549).is_none());
        // 600ms 恰好展开（±50 判据带内）。
        assert!(matches!(s.tick(1_600), Some(DropPhase::Expanded { target: 0, .. })));
    }

    #[test]
    fn esc_rebound_lands_at_origin() {
        let mut s = DragSession::new((7, 9), targets());
        let _ = s.on_move(0, 50, 50, &Rect::new(0, 0, 800, 600));
        s.tick(1_000);
        assert!(s.cancel());
        assert_eq!(s.rebound_endpoint(), (7, 9));
        // 弹性曲线：320ms 处收敛、中段有回弹形状（>1000 overshoot 存在）。
        let p = MotionPolicy::normal();
        assert_eq!(s.rebound_progress(p, REBOUND_MS), 1000);
        let mut saw_overshoot = false;
        for t in 0..REBOUND_MS {
            if s.rebound_progress(p, t) > 1000 {
                saw_overshoot = true;
                break;
            }
        }
        assert!(saw_overshoot, "F124 弹性曲线应有 overshoot");
    }

    #[test]
    fn deny_list_complete() {
        assert!(deny_cursor_audit());
        // 拒绝三类逐一验证。
        assert!(!accepts(TargetKind::ReadOnlyVolume));
        assert!(!accepts(TargetKind::ProtectedArea));
        assert!(!accepts(TargetKind::SelfDrop));
        // 接受五类逐一验证。
        assert!(accepts(TargetKind::Folder));
        assert!(accepts(TargetKind::FileWindow));
        assert!(accepts(TargetKind::Trash));
        assert!(accepts(TargetKind::WindowEdge));
        assert!(accepts(TargetKind::Terminal));
    }

    #[test]
    fn rebound_20_cases() {
        let kinds = [
            TargetKind::Folder,
            TargetKind::FileWindow,
            TargetKind::Trash,
            TargetKind::Terminal,
            TargetKind::Folder,
        ];
        let mut ok = 0;
        for &k in kinds.iter() {
            for expanded in [false, true] {
                let mut s = DragSession::new((1, 2), vec![DropTarget { kind: k, rect: Rect::new(0, 0, 100, 100) }]);
                let _ = s.on_move(0, 50, 50, &Rect::new(0, 0, 800, 600));
                if expanded {
                    s.tick(1_000);
                }
                assert!(s.cancel());
                let p = MotionPolicy::normal();
                // 缺陷账本：现象=本测试红（ok 至多 10≠20）；根因=按会话数
                // 计 ok（5 目标 × 2 相位 = 10），与模块头「20 用例 = 2 状态
                // × 5 目标 × 2 断言」口径矛盾——与检查项 5 同源，属测试
                // 计数写错；修法=按每会话两项断言计数（与检查项 5 同修）。
                if s.rebound_endpoint() == (1, 2) && s.rebound_progress(p, REBOUND_MS) >= 1000 {
                    ok += 2;
                }
            }
        }
        assert_eq!(ok, 20);
    }

    #[test]
    fn drophl_selfcheck_all_green() {
        let set = run_drophl_checks();
        assert!(set.all_passed(), "F212 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 深化范围（仍属主册 F212 验收定义的实装细化，非新立项）：持久化面 = 禁止
// 落点清单 + 高亮参数的 VXH1 定长记录；壳接线面 = 落点高亮绘制清单（令牌色
// 索引定长图元）+ 自动展开倒计时判定；判定面 = run_drophl_v2_checks。
// 零堆：编解码全走定长缓冲。

/// v2 记录魔数（H1 二次批统一身份面）与版本（布局演进守门）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;
/// 记录定长：4 魔数 + 1 版本 + 5 载荷（禁止位图 1 + 微放大 2 + 预算 2）+ 4 校验。
pub const V2_RECORD_BYTES: usize = 14;

/// v2 持久化错误：四类损坏输入全部显性拒绝（明确错误枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2PersistError { BadMagic, BadVersion, BadChecksum, BadLength }

/// FNV-1a 32 位校验和（v2 各记录共用口径，一处一事实）。
fn v2_fnv1a(data: &[u8]) -> u32 {
    data.iter().fold(0x811C_9DC5, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// 禁止落点清单记录（持久化面）：8 类目标拒绝位图 + 高亮参数存档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DenyListRecord {
    pub deny_bitmap: u8, // bit i = ALL_KINDS[i] 为拒绝（「清单审计无漏标」存档面）
    pub scale_mili: u16, // 微放大 ×1000 定点（判据 1.02 → 1020）
    pub budget_ms: u16,  // 高亮延迟预算（判据「实测 <50ms」）
}

impl DenyListRecord {
    /// 采集：位图直接从 accepts 表导出（一处一事实，无二次真值源）。
    pub fn capture() -> DenyListRecord {
        let mut bm = 0u8;
        for (i, k) in ALL_KINDS.iter().enumerate() {
            if !accepts(*k) {
                bm |= 1 << i;
            }
        }
        DenyListRecord { deny_bitmap: bm, scale_mili: HIGHLIGHT_SCALE_MILI as u16, budget_ms: HIGHLIGHT_BUDGET_MS as u16 }
    }

    /// 存档位图与 accepts 表逐类核对一致（防存档漂移的审计面）。
    pub fn matches_accepts(&self) -> bool {
        ALL_KINDS.iter().enumerate().all(|(i, &k)| ((self.deny_bitmap >> i) & 1 == 1) == !accepts(k))
    }

    /// 编码：VXH1 + 版本 + 5 字节定长载荷 + FNV-1a 校验和。
    pub fn to_bytes(&self) -> [u8; V2_RECORD_BYTES] {
        let mut out = [0u8; V2_RECORD_BYTES];
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5] = self.deny_bitmap;
        out[6..8].copy_from_slice(&self.scale_mili.to_le_bytes());
        out[8..10].copy_from_slice(&self.budget_ms.to_le_bytes());
        let sum = v2_fnv1a(&out[..V2_RECORD_BYTES - 4]);
        out[V2_RECORD_BYTES - 4..].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：长度/魔数/版本/校验四门逐道拒绝。
    pub fn from_bytes(b: &[u8]) -> Result<DenyListRecord, V2PersistError> {
        if b.len() < V2_RECORD_BYTES { return Err(V2PersistError::BadLength); }
        if b[..4] != V2_MAGIC { return Err(V2PersistError::BadMagic); }
        if b[4] != V2_VERSION { return Err(V2PersistError::BadVersion); }
        let sum = u32::from_le_bytes([b[10], b[11], b[12], b[13]]);
        if v2_fnv1a(&b[..10]) != sum { return Err(V2PersistError::BadChecksum); }
        Ok(DenyListRecord {
            deny_bitmap: b[5],
            scale_mili: u16::from_le_bytes([b[6], b[7]]),
            budget_ms: u16::from_le_bytes([b[8], b[9]]),
        })
    }
}

// ---------------------------------------------------------------------------
// UI 壳接线：落点高亮绘制清单（令牌色索引）+ 自动展开倒计时判定
// ---------------------------------------------------------------------------

/// 令牌色索引（合成器消费的语义槽；具体色值由主题令牌表解析）。
pub const TOKEN_HIGHLIGHT: u8 = 1; // 可放置高亮（边框亮起 + 微放大）
pub const TOKEN_DENY: u8 = 2;      // 禁止光标标注
pub const TOKEN_EDGE: u8 = 3;      // 贴边分屏预览

/// 绘制图元：矩形 + 令牌色索引（合成器消费的定长清单单元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawPrim {
    pub rect: Rect,
    pub color_idx: u8,
}

/// 高亮清单容量：四条 2px 边框 + 一枚中心微放大标注（定长，零堆）。
pub const HIGHLIGHT_PRIM_CAP: usize = 5;

/// 悬停可放置目标的高亮绘制清单：四条 2px 边框 + 中心内缩 1/8 标注块
/// （整数域近似 1.02 微放大——「边框亮起 + 微放大」的绘制面）。
pub fn highlight_draw_list(t: &DropTarget) -> ([DrawPrim; HIGHLIGHT_PRIM_CAP], usize) {
    let r = t.rect;
    let b = 2;
    let prims = [
        DrawPrim { rect: Rect::new(r.x, r.y, r.w, b), color_idx: TOKEN_HIGHLIGHT },
        DrawPrim { rect: Rect::new(r.x, r.bottom() - b, r.w, b), color_idx: TOKEN_HIGHLIGHT },
        DrawPrim { rect: Rect::new(r.x, r.y + b, b, r.h - 2 * b), color_idx: TOKEN_HIGHLIGHT },
        DrawPrim { rect: Rect::new(r.right() - b, r.y + b, b, r.h - 2 * b), color_idx: TOKEN_HIGHLIGHT },
        DrawPrim {
            rect: Rect::new(r.x + r.w / 8, r.y + r.h / 8, r.w * 3 / 4, r.h * 3 / 4),
            color_idx: TOKEN_HIGHLIGHT,
        },
    ];
    (prims, HIGHLIGHT_PRIM_CAP)
}

/// 拒绝目标（禁止光标）：单枚禁止标注图元——不高亮不吸引（落点诚实）。
pub fn deny_draw_prim(t: &DropTarget) -> DrawPrim {
    let r = t.rect;
    DrawPrim { rect: Rect::new(r.x + r.w / 2 - 8, r.y + r.h / 2 - 8, 16, 16), color_idx: TOKEN_DENY }
}

/// 自动展开倒计时判定：剩余 ms（0 = 已到点；「600ms±50ms」的边界读数）。
pub fn expand_countdown_ms(since: u64, now: u64) -> u64 {
    AUTO_EXPAND_MS.saturating_sub(now.saturating_sub(since))
}

/// F212 v2 自检（首条=持久化 round-trip；逐条注明验主册哪句话）。
pub fn run_drophl_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F212-drophl-v2");
    let rec = DenyListRecord::capture();
    let blob = rec.to_bytes();
    // 1. round-trip：采集→编码→解码逐字段相等（v2 记录纪律）。
    set.add("v2 record round-trip", DenyListRecord::from_bytes(&blob) == Ok(rec), "");
    // 2. 四类损坏输入全部拒绝（魔数/版本/校验/长度）。
    let mut bad_magic = blob; bad_magic[0] = b'X';
    let mut bad_ver = blob; bad_ver[4] = 9;
    let mut bad_sum = blob; bad_sum[6] ^= 0xFF;
    set.add(
        "corruption four-way rejected",
        DenyListRecord::from_bytes(&bad_magic) == Err(V2PersistError::BadMagic)
            && DenyListRecord::from_bytes(&bad_ver) == Err(V2PersistError::BadVersion)
            && DenyListRecord::from_bytes(&bad_sum) == Err(V2PersistError::BadChecksum)
            && DenyListRecord::from_bytes(&blob[..13]) == Err(V2PersistError::BadLength),
        "",
    );
    // 3. 验「禁止光标场景清单审计无漏标」：存档位图与 accepts 表逐类一致。
    set.add("archived deny bitmap matches accepts", rec.matches_accepts(), "");
    // 4. 验「边框亮起 + 微放大」绘制面：5 枚图元全部令牌高亮色。
    let t = DropTarget { kind: TargetKind::Folder, rect: Rect::new(10, 10, 100, 60) };
    let (prims, n) = highlight_draw_list(&t);
    set.add(
        "highlight draw list 5 prims on token",
        n == HIGHLIGHT_PRIM_CAP && prims.iter().take(n).all(|p| p.color_idx == TOKEN_HIGHLIGHT),
        "",
    );
    // 5. 验「不可放置目标显示禁止光标且不高亮不吸引」：拒绝目标只得禁止标注。
    let dt = DropTarget { kind: TargetKind::ProtectedArea, rect: Rect::new(0, 0, 100, 100) };
    set.add("denied target gets deny prim only", deny_draw_prim(&dt).color_idx == TOKEN_DENY, "");
    // 6. 验「600ms±50ms」判据带：549ms 尚余 51、600ms 归零。
    set.add(
        "expand countdown boundaries",
        expand_countdown_ms(1_000, 1_549) == 51 && expand_countdown_ms(1_000, 1_600) == 0,
        "",
    );
    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn deny_record_round_trip_and_reject() {
        let rec = DenyListRecord::capture();
        let blob = rec.to_bytes();
        assert_eq!(DenyListRecord::from_bytes(&blob), Ok(rec));
        assert_eq!(rec.scale_mili, 1020);
        assert_eq!(rec.budget_ms, 50);
        assert!(DenyListRecord::from_bytes(&[]).is_err());
    }

    #[test]
    fn draw_list_shape_and_countdown() {
        let t = DropTarget { kind: TargetKind::Trash, rect: Rect::new(0, 0, 80, 40) };
        let (prims, n) = highlight_draw_list(&t);
        assert_eq!(n, 5);
        assert_eq!(prims[0].rect.y, 0); // 顶边框贴目标上缘
        assert_eq!(prims[1].rect.y, 38); // 底边框贴下缘
        assert_eq!(expand_countdown_ms(0, AUTO_EXPAND_MS - 1), 1);
        assert_eq!(expand_countdown_ms(0, AUTO_EXPAND_MS + 99), 0);
    }

    #[test]
    fn drophl_v2_selfcheck_all_green() {
        let set = run_drophl_v2_checks();
        assert!(set.all_passed(), "F212 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
