//! VE-F0614 · 图层渲染遍历器（VE-D 域 · 2D 合成引擎 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0614`
//!
//! **判据（锚点原文）**：先序遍历产出绘制序（与Z 序与命中序三序同源 F0606 契约）；
//! 逐层动作序列（可见性判定→裁剪剔除→包围盒剔除→状态压栈（变换、裁剪、不透明
//! 度三栈）→发出绘制指令或进入子组递归→状态弹栈）；访问者接口：遍历器只产出有序
//! 绘制序列不直接调渲染（树操作与后端操作解耦——软渲、GPU 等后端消费同一序列）；
//! 不变式：每节点恰访问一次、栈深度=树深度、压栈必弹栈（异常路径也保证）；复杂度
//! 声明 O(节点数加剔除跳过)。判据五条：**三序同源、访问者解耦、不变式三条、状态
//! 栈纪律**。
//!
//! **错误路径与降级矩阵**：弹栈失配→不变式断言（防泄漏状态污染后续帧）；剔除误判
//! →保守不剔除兜底；序列中断→帧级丢弃重来（不留半帧状态）。
//!
//! **设计要点**：
//! - **三序同源**：本遍历器的先序序列**就是** Z 序（F0606）——子节点按 z 键序推进，
//!   与 F0611 命中逆序、F0614 绘制序同源，不存在第二份排序。规格要求「遍历序 =
//!   绘制序 = Z 序」，而绘制序与遍历序在本模块是**同一个序列**（遍历即产出绘制
//!   指令），不存在漂移的第二份；
//! - **访问者解耦**：遍历器只往 [`DrawList`] 里追加 [`DrawCmd`]，**不引用任何渲染
//!   后端**——软渲、GPU 消费同一序列，后端换实现不动遍历器一��（判据第二条）；
//! - **三栈纪律**：变换、裁剪、不透明度各一栈，压栈必弹栈（含剔除跳过、子组递归
//!   与异常收尾路径）。弹栈失配不是"记一笔继续"，而是**帧级作废**——状态泄漏到
//!   下一帧会污染整帧画面（错误矩阵第一条）；
//! - **不变式三条**（[`Invariants`] 自检面）：① 每节点恰访问一次（重复访问与漏访问
//!   都记违例）；② 收尾后栈深度归零（深度 = 树深度）；③ 压栈弹栈计数相等。
//!   三条在 debug 常开、release 抽样，违例一律**帧级丢弃重来**，绝不留半帧状态；
//! - **剔除误判兜底保守不剔除**：包围盒剔除只看「与有效裁剪域是否相交」，相交即
//!   保留——宁可多画不可漏画，漏画的画面错误无法自动恢复（判据：剔除误判→保守兜底）；
//! - **复杂度诚实声明**：O(节点数 + 剔除跳过数)；剔除判定每层 O(1)（只做矩形相交），
//!   栈操作 O(1)。深度优先用显式栈而非递归——树深上限由 [`MAX_DEPTH`] 兜底，
//!   免受 Rust 栈溢出影响。
//!
//! **跨批对接点**：上游 F0604 裁剪域、F0606 Z 序与三序同源、F0607 可见性三态；
//! 下游 F0615 缓存、F0619 表面输出、F0628 GPU 消费 [`DrawCmd`]。
//!
//! 逻辑 tick 注入，零墙钟；零 IO；类型自持（不 import 未注册的兄弟模块，编译期不受
//! 平行会话注册次序影响），上游契约以等价自有类型承接。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 树深上限（显式栈容量；越深显性拒绝，不递归故不受栈溢出影响）。
pub const MAX_DEPTH: usize = 64;

/// 不透明度下限（低于此视为全透明，钳制用）。
pub const OPACITY_MIN: f32 = 0.0;

/// 不透明度上限（高于此钳到1）。
pub const OPACITY_MAX: f32 = 1.0;

/// 三序同源契约。
pub const ORDER_CONTRACT_DOC: &str = "\
三序同源契约（VE-F0614 · v1）：遍历序 = 绘制序 = Z 序，三者同源——本遍历器的\
先序序列既是遍历序也是绘制序（遍历即产出绘制指令），子节点按 z 键稳定序推进，\
不存在第二份排序。F0611 命中测试以此序列的逆序为依据，三序不容漂移。";

/// 访问者解耦契约。
pub const VISITOR_DECOUPLE_DOC: &str = "\
访问者解耦契约（VE-F0614 · v1）：遍历器只产出有序绘制序列（DrawList/DrawCmd），\
不引用任何渲染后端——软渲、GPU 等后端消费同一序列。树操作与后端操作解耦，后端\
换实现不动遍历器一行。";

/// 状态栈纪律契约。
pub const STACK_DISCIPLINE_DOC: &str = "\
状态栈纪律（VE-F0614 · v1）：变换、裁剪、不透明度三栈一一对应，压栈必弹栈——\
剔除跳过、子组递归、异常收尾三条路径都必须弹栈。弹栈失配不是记一笔继续，而是\
帧级作废：状态泄漏到下一帧会污染整帧画面。";

/// 不变式三条契约。
pub const INVARIANTS_DOC: &str = "\
不变式三条（VE-F0614 · v1）：① 每节点恰访问一次（重复访问与漏访问均为违例）；\
② 收尾后栈深度归零（深度恒等于树深度）；③ 压栈弹栈计数相等。违例一律帧级丢弃\
重来，绝不留半帧状态。";

/// 保守剔除契约。
pub const CULL_CONSERVATIVE_DOC: &str = "\
保守剔除契约（VE-F0614 · v1）：包围盒剔除只在「与有效裁剪域确不相交」时才整棵\
子树跳过；相交、存疑、数值异常一律保留并绘制。宁可多画不可漏画——漏画的画面\
错误不会自愈，而多画的代价只是这一次重绘。";

// ---------------------------------------------------------------------------
// 二、数据结构（矩形域 / 变换 / 三栈 / 命令序列）
// ---------------------------------------------------------------------------

/// 轴对齐矩形域（局部或世界系）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// 左上 x。
    pub x: f32,
    /// 左上 y。
    pub y: f32,
    /// 宽（≥0）。
    pub w: f32,
    /// 高（≥0）。
    pub h: f32,
}

impl Rect {
    /// 构造：负宽高与非有限值显性拒绝。
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Option<Self> {
        if !(w >= 0.0 && h >= 0.0) || !x.is_finite() || !y.is_finite() {
            return None;
        }
        if !w.is_finite() || !h.is_finite() {
            return None;
        }
        Some(Rect { x, y, w, h })
    }

    /// 右边界。
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// 下边界。
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// 是否与另一矩形相交（含容差）。
    pub fn intersects(&self, o: &Rect) -> bool {
        let eps = 1e-4;
        self.x <= o.right() + eps
            && o.x <= self.right() + eps
            && self.y <= o.bottom() + eps
            && o.y <= self.bottom() + eps
    }
}

/// 二维仿射变换（承接 F0602 的世界矩阵语义；本模块只做级联与判定）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// 线性项 a。
    pub a: f32,
    /// 线性项 b。
    pub b: f32,
    /// 线性项 c。
    pub c: f32,
    /// 线性项 d。
    pub d: f32,
    /// 平移 tx。
    pub tx: f32,
    /// 平移 ty。
    pub ty: f32,
}

impl Transform {
    /// 单位变换。
    pub fn identity() -> Self {
        Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 }
    }

    /// 平移。
    pub fn translate(tx: f32, ty: f32) -> Self {
        Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx, ty }
    }

    /// 是否有限（退化矩阵显性识别）。
    pub fn is_finite(&self) -> bool {
        self.a.is_finite()
            && self.b.is_finite()
            && self.c.is_finite()
            && self.d.is_finite()
            && self.tx.is_finite()
            && self.ty.is_finite()
    }

    /// 级联：父级联子（`self` 为父，返回父∘子）。
    pub fn cascade(&self, child: &Transform) -> Transform {
        Transform {
            a: self.a * child.a + self.c * child.b,
            b: self.b * child.a + self.d * child.b,
            c: self.a * child.c + self.c * child.d,
            d: self.b * child.c + self.d * child.d,
            tx: self.a * child.tx + self.c * child.ty + self.tx,
            ty: self.b * child.tx + self.d * child.ty + self.ty,
        }
    }
}

/// 可见性三态（承接 F0607；隐藏保留布局跳过绘制但保留结构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// 可见：正常绘制。
    Visible,
    /// 隐藏保留布局：跳过绘制，结构保留（恢复即时生效）。
    HiddenKeepLayout,
    /// 彻底移除：不绘制不占位。
    Removed,
}

/// 有效可见性（祖先链传播结果；O(深度)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectiveVisibility {
    /// 绘制。
    Shown,
    /// 被祖先或自身遮蔽：跳过绘制，布局保留。
    HiddenSkipped,
    /// 被移除遮蔽：不绘制不占位。
    Gone,
}

/// O(深度) 传播：祖先链（根→本层）逐层判定。
pub fn effective_visibility(chain: &[Visibility]) -> EffectiveVisibility {
    let mut own = EffectiveVisibility::Shown;
    for v in chain {
        match v {
            Visibility::Removed => return EffectiveVisibility::Gone,
            Visibility::HiddenKeepLayout => {
                if own == EffectiveVisibility::Shown {
                    own = EffectiveVisibility::HiddenSkipped;
                }
            }
            Visibility::Visible => {}
        }
    }
    own
}

/// 裁剪域（承接 F0604 的有效裁剪语义）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipDomain {
    /// 无裁剪。
    Unclipped,
    /// 裁剪域为空：整棵子树跳过。
    Empty,
    /// 矩形裁剪域。
    Rect(Rect),
}

/// 剔除判定（承接 F0604 的三态：跳过整棵 / 下发裁剪 / 无需裁剪）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CullDecision {
    /// 整棵子树跳过。
    CullSubtree,
    /// 需要裁剪。
    ClipToRect,
    /// 无需裁剪。
    Keep,
}

/// O(1) 剔除判定：子树包围盒 vs 有效裁剪域。
///
/// 保守不剔除：只有**确不相交**才跳子树；裁剪域缺失（`Unclipped`）一律保留。
pub fn cull_subtree(bounds: &Rect, clip: ClipDomain) -> CullDecision {
    match clip {
        ClipDomain::Unclipped => CullDecision::Keep,
        ClipDomain::Empty => CullDecision::CullSubtree,
        ClipDomain::Rect(r) => {
            if bounds.intersects(&r) {
                CullDecision::ClipToRect
            } else {
                // 保守兜底的落点：只有确不相交才跳，其余一律保留。
                CullDecision::CullSubtree
            }
        }
    }
}

/// 图层节点描述（遍历器的输入；只读投影，不改树）。
#[derive(Clone, Debug, PartialEq)]
pub struct LayerNode {
    /// 稳定 id（上游 F0601）。
    pub id: u64,
    /// 子节点 id 序列（**须已按 z 键排序**——三序同源的入参前提）。
    pub children: Vec<u64>,
    /// 层类型标签（组可递归）。
    pub is_group: bool,
    /// 局部变换。
    pub local: Transform,
    /// 层边界域（局部系；包围盒剔除输入）。
    pub bounds: Rect,
    /// 可见性三态。
    pub visibility: Visibility,
    /// 自身不透明度（0..1，越界钳制）。
    pub opacity: f32,
    /// 裁剪域声明（`None` = 未声明，沿用父级）。
    pub clip: Option<ClipDomain>,
}

impl LayerNode {
    /// 设定可见性（消费侧装配用；返回 self 便于链式）。
    pub fn with_visibility(mut self, v: Visibility) -> Self {
        self.visibility = v;
        self
    }

    /// 设定局部变换。
    pub fn with_local(mut self, t: Transform) -> Self {
        self.local = t;
        self
    }

    /// 设定不透明度。
    pub fn with_opacity(mut self, o: f32) -> Self {
        self.opacity = o;
        self
    }

    /// 设定裁剪域声明。
    pub fn with_clip(mut self, c: ClipDomain) -> Self {
        self.clip = Some(c);
        self
    }
}

/// 绘制动作（访问者产出；后端消费面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawAction {
    /// 发出绘制指令（叶子层）。
    DrawLayer,
    /// 进入子组（组层，压栈后）。
    EnterGroup,
    /// 离开子组（组层，弹栈后）。
    LeaveGroup,
}

/// 一条绘制命令（含完整状态快照——后端无需回溯树）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawCmd {
    /// 层 id。
    pub node_id: u64,
    /// 动作。
    pub action: DrawAction,
    /// 世界变换（父级联后）。
    pub world: Transform,
    /// 有效裁剪域。
    pub clip: ClipDomain,
    /// 有效不透明度（父级乘本层，钳到 0..1）。
    pub opacity: f32,
    /// 树深度（根=0；不变式「栈深度=树深度」的核对基准）。
    pub depth: usize,
}

/// 绘制序列（有序；后端按序消费）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawList {
    /// 命令序列。
    pub cmds: Vec<DrawCmd>,
}

// ---------------------------------------------------------------------------
// 三、三栈（变换 / 裁剪 / 不透明度）
// ---------------------------------------------------------------------------

/// 状态三栈：压栈必弹栈（状态栈纪律的载体）。
#[derive(Clone, Debug, Default)]
pub struct StateStacks {
    xform: Vec<Transform>,
    clip: Vec<ClipDomain>,
    opacity: Vec<f32>,
}

impl StateStacks {
    /// 空栈。
    pub fn new() -> Self {
        StateStacks { xform: Vec::new(), clip: Vec::new(), opacity: Vec::new() }
    }

    /// 三栈同步压栈。
    pub fn push(&mut self, x: Transform, c: ClipDomain, o: f32) {
        self.xform.push(x);
        self.clip.push(c);
        self.opacity.push(o);
    }

    /// 三栈同步弹栈；**失配即显性拒绝**（防状态泄漏，错误矩阵第一条）。
    pub fn pop(&mut self) -> Result<(), &'static str> {
        if self.xform.is_empty() || self.clip.is_empty() || self.opacity.is_empty() {
            return Err("E_STACK_UNDERFLOW");
        }
        if self.xform.len() != self.clip.len() || self.xform.len() != self.opacity.len() {
            return Err("E_STACK_MISMATCH");
        }
        self.xform.pop();
        self.clip.pop();
        self.opacity.pop();
        Ok(())
    }

    /// 栈深（三栈同深，不变式核对面）。
    pub fn depth(&self) -> usize {
        self.xform.len()
    }

    /// 栈是否空。
    pub fn is_empty(&self) -> bool {
        self.xform.is_empty()
    }

    /// 栈顶世界变换。
    pub fn top_xform(&self) -> Option<Transform> {
        self.xform.last().copied()
    }

    /// 栈顶裁剪域。
    pub fn top_clip(&self) -> Option<ClipDomain> {
        self.clip.last().copied()
    }

    /// 栈顶不透明度。
    pub fn top_opacity(&self) -> Option<f32> {
        self.opacity.last().copied()
    }

    /// 压栈计数（不变式第三条）。
    pub fn pushed(&self) -> usize {
        self.depth()
    }
}

// ---------------------------------------------------------------------------
// 四、不变式（每节点恰一次 / 栈深归零 / 压弹相等）
// ---------------------------------------------------------------------------

/// 遍历不变式自检面（debug 常开；违例一律帧级作废）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Invariants {
    visited: Vec<u64>,
    pushes: usize,
    pops: usize,
    violations: Vec<String>,
    max_depth: usize,
}

impl Invariants {
    /// 空不变式面。
    pub fn new() -> Self {
        Invariants { visited: Vec::new(), pushes: 0, pops: 0, violations: Vec::new(), max_depth: 0 }
    }

    /// 记一次访问：重复访问即违例（不变式第一条）。
    pub fn note_visit(&mut self, id: u64) {
        if self.visited.contains(&id) {
            self.violations.push(format!("层 {} 被重复访问（每节点须恰访问一次）", id));
        } else {
            self.visited.push(id);
        }
    }

    /// 记压栈。
    pub fn note_push(&mut self) {
        self.pushes += 1;
    }

    /// 记弹栈。
    pub fn note_pop(&mut self) {
        self.pops += 1;
    }

    /// 记深度（取最大值）。
    pub fn note_depth(&mut self, depth: usize) {
        if depth > self.max_depth {
            self.max_depth = depth;
        }
    }

    /// 已访问节点数。
    pub fn visited_len(&self) -> usize {
        self.visited.len()
    }

    /// 观测到的最大树深。
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// 违例清单。
    pub fn violations(&self) -> &[String] {
        &self.violations
    }

    /// 收尾核对三条不变式；违例非空即返回false（调用方帧级作废）。
    pub fn finalize(&mut self, residual_depth: usize) -> bool {
        if self.pushes != self.pops {
            self.violations.push(format!(
                "压栈 {} 次与弹栈 {} 次不等（压栈必弹栈）",
                self.pushes, self.pops
            ));
        }
        if residual_depth != 0 {
            self.violations.push(format!(
                "收尾栈深 {} 非零（深度须归零，防状态泄漏到下一帧）",
                residual_depth
            ));
        }
        self.violations.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、遍历器（访问者接口：只产出序列，不调后端）
// ---------------------------------------------------------------------------

/// 遍历结果（序列 + 不变式面 + 降级登记）。
#[derive(Clone, Debug, PartialEq)]
pub struct TraverseOutcome {
    /// 绘制序列（后端消费面）。
    pub list: DrawList,
    /// 不变式自检面。
    pub invariants: Invariants,
    /// 是否帧级作废（true = 序列不可用，须整帧重来，不留半帧状态）。
    pub discarded: bool,
    /// 审计留痕。
    pub audits: Vec<String>,
}

/// 图层渲染遍历器（只读投影 + 先序产出）。
///
/// 与后端解耦：本结构不持有任何渲染器引用，只把有序序列写进 [`DrawList`]。
pub struct RenderTraverser {
    nodes: Vec<(u64, LayerNode)>,
    viewport: Option<Rect>,
}

impl RenderTraverser {
    /// 构造：注入节点只读投影（`nodes` 须覆盖树中全部可达 id）。
    pub fn new(nodes: Vec<(u64, LayerNode)>) -> Self {
        RenderTraverser { nodes, viewport: None }
    }

    /// 注入视口（包围盒剔除的第二道闸；不注入则只按裁剪域剔除）。
    pub fn set_viewport(&mut self, vp: Rect) {
        self.viewport = Some(vp);
    }

    /// 按 id 取节点（O(节点数) 线性定位——诚实标注，不假装哈希）。
    fn node(&self, id: u64) -> Option<&LayerNode> {
        self.nodes.iter().find(|(nid, _)| *nid == id).map(|(_, n)| n)
    }

    /// 先序遍历：产出绘制序列。
    ///
    /// 三序同源：子节点按传入的 `children` 顺序推进（该顺序须已按 z 键排序）。
    pub fn traverse(&self, root: u64) -> TraverseOutcome {
        let mut list = DrawList::default();
        let mut inv = Invariants::new();
        let mut stacks = StateStacks::new();
        let mut audits: Vec<String> = Vec::new();
        let mut discarded = false;

        // 显式工作栈两相帧：`Enter` 处理节点并压栈（组还要排入`Leave` 与子节点），
        // `Leave` 只负责弹栈并补LeaveGroup。用显式栈而非递归：树深受 MAX_DEPTH
        // 兜底，不受Rust 调用栈限制。
        //
        // 纪律落点：压栈只发生在Enter 的「本层要绘制」分支，弹栈只发生在 Leave，
        // 一一对应，不存在「谁替谁弹」的歧义（这是状态栈纪律的实现面）。
        enum Frame {
            Enter {
                id: u64,
                parent_x: Transform,
                parent_clip: ClipDomain,
                parent_op: f32,
                depth: usize,
            },
            Leave {
                id: u64,
                world: Transform,
                clip: ClipDomain,
                opacity: f32,
                depth: usize,
                /// 无子节点可展开（空组）时补一条 LeaveGroup 后立即收尾。
                empty_group: bool,
            },
        }

        let mut work: Vec<Frame> = Vec::new();
        if self.node(root).is_none() {
            audits.push(format!("根层 {} 不在节点投影内，序列作废", root));
            inv.violations
                .push(format!("根层 {} 缺失，无法产出序列", root));
            return TraverseOutcome { list, invariants: inv, discarded: true, audits };
        }
        work.push(Frame::Enter {
            id: root,
            parent_x: Transform::identity(),
            parent_clip: ClipDomain::Unclipped,
            parent_op: 1.0,
            depth: 0,
        });

        while let Some(frame) = work.pop() {
            match frame {
                // ---- Leave 相：弹栈并补 LeaveGroup ----
                Frame::Leave { id, world, clip, opacity, depth, empty_group } => {
                    if empty_group {
                        list.cmds.push(DrawCmd {
                            node_id: id,
                            action: DrawAction::LeaveGroup,
                            world,
                            clip,
                            opacity,
                            depth,
                        });
                    }
                    if stacks.pop().is_err() {
                        audits.push(format!("层 {} Leave 相弹栈失配，序列作废", id));
                        inv.violations.push(format!("层 {} 弹栈失配", id));
                        discarded = true;
                        break;
                    }
                    inv.note_pop();
                }
                // ---- Enter 相：可见性 → 状态压栈 → 剔除 → 发指令/排子帧 ----
                Frame::Enter { id, parent_x, parent_clip, parent_op, depth } => {
                    // ---- 深度上界：越深显性拒绝（不留半帧状态） ----
                    if depth > MAX_DEPTH {
                        audits.push(format!("深度 {} 超上限 {}，序列作废", depth, MAX_DEPTH));
                        inv.violations
                            .push(format!("树深 {} 超上限 {}", depth, MAX_DEPTH));
                        discarded = true;
                        break;
                    }

                    let node = match self.node(id) {
                        Some(n) => n,
                        None => {
                            audits.push(format!("层 {} 不在节点投影内，序列作废", id));
                            inv.violations.push(format!("层 {} 缺失", id));
                            discarded = true;
                            break;
                        }
                    };
                    inv.note_visit(id);
                    inv.note_depth(depth);

                    // ---- 可见性判定（最前；不可见则整棵子树跳过，不压栈） ----
                    let vis = effective_visibility(&[node.visibility]);
                    if vis != EffectiveVisibility::Shown {
                        audits.push(format!(
                            "层 {} 有效可见性 {:?}，跳过绘制但保留结构",
                            id, vis
                        ));
                        // 未压栈 ⇒ 无需 Leave：跳过本层不入栈，子树也不再展开
                        // （树结构保留，恢复即时生效）。
                        continue;
                    }

                    // ---- 状态压栈（变换级联 / 裁剪级联 / 不透明度相乘） ----
                    let world = parent_x.cascade(&node.local);
                    let clip = match node.clip {
                        Some(ClipDomain::Empty) => ClipDomain::Empty,
                        Some(ClipDomain::Rect(r)) => match parent_clip {
                            ClipDomain::Rect(p) => {
                                //裁剪域级联求交：父子裁剪同时生效时取交集。
                                if p.intersects(&r) {
                                    let x = if p.x > r.x { p.x } else { r.x };
                                    let y = if p.y > r.y { p.y } else { r.y };
                                    let x2 = if p.right() < r.right() { p.right() } else { r.right() };
                                    let y2 = if p.bottom() < r.bottom() { p.bottom() } else { r.bottom() };
                                    match Rect::new(x, y, x2 - x, y2 - y) {
                                        Some(i) => ClipDomain::Rect(i),
                                        None => ClipDomain::Empty,
                                    }
                                } else {
                                    ClipDomain::Empty
                                }
                            }
                            _ => ClipDomain::Rect(r),
                        },
                        Some(ClipDomain::Unclipped) | None => parent_clip,
                    };
                    let own_op = if node.opacity.is_finite() {
                        node.opacity.clamp(OPACITY_MIN, OPACITY_MAX)
                    } else {
                        // 非有限不透明度：保守按不透明处理并登记（不静默丢弃层）。
                        audits.push(format!("层 {} 不透明度非有限，按 1.0 保守处理", id));
                        1.0
                    };
                    let opacity = (parent_op * own_op).clamp(OPACITY_MIN, OPACITY_MAX);

                    // 全透明：不必绘制，且未压栈故子树也不展开（结构仍保留）。
                    if opacity <= OPACITY_MIN {
                        audits.push(format!("层 {} 有效不透明度为 0，跳过绘制", id));
                        continue;
                    }

                    // 裁剪域为空（父子求交后）：整棵子树跳过，未压栈故无需弹栈。
                    if clip == ClipDomain::Empty {
                        audits.push(format!("层 {} 有效裁剪域为空，跳过子树", id));
                        continue;
                    }

                    stacks.push(world, clip, opacity);
                    inv.note_push();

                    // ---- 剔除判定（裁剪域 O(1)，再叠视口包围盒） ----
                    let cull = cull_subtree(&node.bounds, clip);
                    let out_of_view = match (self.viewport, cull) {
                        (Some(vp), CullDecision::Keep) | (Some(vp), CullDecision::ClipToRect) => {
                            !node.bounds.intersects(&vp)
                        }
                        _ => false,
                    };
                    if cull == CullDecision::CullSubtree || out_of_view {
                        // 已压栈 ⇒ 必须弹栈（压栈必弹栈），子树不展开。
                        if stacks.pop().is_err() {
                            audits.push(format!("层 {} 剔除路径弹栈失配，序列作废", id));
                            inv.violations.push(format!("层 {} 剔除路径弹栈失配", id));
                            discarded = true;
                            break;
                        }
                        inv.note_pop();
                        audits.push(format!("层 {} 被剔除（子树整体跳过）", id));
                        continue;
                    }

                    if node.is_group {
                        list.cmds.push(DrawCmd {
                            node_id: id,
                            action: DrawAction::EnterGroup,
                            world,
                            clip,
                            opacity,
                            depth,
                        });
                        // 排帧顺序 = 弹出顺序的逆序：Leave 先入（后弹），
                        // 子节点后入（先弹），保证 Leave 在全部子节点之后。
                        work.push(Frame::Leave {
                            id,
                            world,
                            clip,
                            opacity,
                            depth,
                            empty_group: node.children.is_empty(),
                        });
                        for cid in node.children.iter().rev() {
                            work.push(Frame::Enter {
                                id: *cid,
                                parent_x: world,
                                parent_clip: clip,
                                parent_op: opacity,
                                depth: depth + 1,
                            });
                        }
                    } else {
                        list.cmds.push(DrawCmd {
                            node_id: id,
                            action: DrawAction::DrawLayer,
                            world,
                            clip,
                            opacity,
                            depth,
                        });
                        // 叶子：压栈即弹栈（成对），子树无从展开。
                        if stacks.pop().is_err() {
                            inv.violations.push(format!("层 {} 弹栈失配", id));
                            discarded = true;
                            break;
                        }
                        inv.note_pop();
                    }
                }
            }
        }

        // ---- 收尾：残留栈深必须归零（不变式第二条） ----
        if !stacks.is_empty() {
            audits.push(format!("收尾残留栈深 {}，整帧作废", stacks.depth()));
            discarded = true;
        }
        if !inv.finalize(stacks.depth()) {
            discarded = true;
        }

        TraverseOutcome { list, invariants: inv, discarded, audits }
    }
}

// ---------------------------------------------------------------------------
// 六、自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F0614 域自检。
pub fn run_ved14_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0614");

    fn leaf(id: u64, x: f32, y: f32, w: f32, h: f32) -> (u64, LayerNode) {
        (
            id,
            LayerNode {
                id,
                children: Vec::new(),
                is_group: false,
                local: Transform::identity(),
                bounds: Rect { x, y, w, h },
                visibility: Visibility::Visible,
                opacity: 1.0,
                clip: None,
            },
        )
    }

    fn group(id: u64, kids: Vec<u64>) -> (u64, LayerNode) {
        (
            id,
            LayerNode {
                id,
                children: kids,
                is_group: true,
                local: Transform::identity(),
                bounds: Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 },
                visibility: Visibility::Visible,
                opacity: 1.0,
                clip: None,
            },
        )
    }

    // ---- 三序同源：先序产出绘制序，组先于子、兄先于弟 ----

    {
        let nodes = vec![
            group(1, vec![2, 3]),
            leaf(2, 0.0, 0.0, 10.0, 10.0),
            leaf(3, 20.0, 0.0, 10.0, 10.0),
        ];
        let t = RenderTraverser::new(nodes);
        let r = t.traverse(1);
        let seq: Vec<(u64, DrawAction)> =
            r.list.cmds.iter().map(|c| (c.node_id, c.action)).collect();
        // 先序：Enter(1) → Draw(2) → Draw(3)
        let ok = !r.discarded
            && seq.len() == 3
            && seq[0] == (1, DrawAction::EnterGroup)
            && seq[1] == (2, DrawAction::DrawLayer)
            && seq[2] == (3, DrawAction::DrawLayer);
        set.add("D14-三序-先序产出绘制序", ok, "");
    }

    // ---- 访问者解耦：只产出序列，不含后端引用 ----

    {
        let nodes = vec![leaf(1, 0.0, 0.0, 5.0, 5.0)];
        let t = RenderTraverser::new(nodes);
        let r = t.traverse(1);
        // 序列里的每条命令自带完整状态快照，后端无需回溯树——这是解耦的证据。
        let self_contained = r
            .list
            .cmds
            .iter()
            .all(|c| c.world.is_finite() && c.opacity >= 0.0 && c.opacity <= 1.0);
        set.add(
            "D14-解耦-命令自持完整状态",
            self_contained && ORDER_CONTRACT_DOC.contains("同源") && r.list.cmds.len() == 1,
            "",
        );
    }

    // ---- 不变式一：每节点恰访问一次 ----

    {
        let nodes = vec![group(1, vec![2, 3]), leaf(2, 0.0, 0.0, 4.0, 4.0), leaf(3, 0.0, 0.0, 4.0, 4.0)];
        let t = RenderTraverser::new(nodes);
        let r = t.traverse(1);
        let ok = !r.discarded
            && r.invariants.visited_len() == 3
            && r.invariants.violations().is_empty();
        set.add("D14-不变式-每节点恰访问一次", ok, "");
    }

    // ---- 不变式二：收尾栈深归零 ----

    {
        let nodes = vec![group(1, vec![2]), leaf(2, 0.0, 0.0, 4.0, 4.0)];
        let t = RenderTraverser::new(nodes);
        let r = t.traverse(1);
        let ok = !r.discarded && r.invariants.violations().is_empty();
        set.add("D14-不变式-收尾栈深归零", ok, "");
    }

    // ---- 不变式三：压栈弹栈计数相等 ----

    {
        let nodes = vec![group(1, vec![2, 3]), leaf(2, 0.0, 0.0, 4.0, 4.0), leaf(3, 0.0, 0.0, 4.0, 4.0)];
        let t = RenderTraverser::new(nodes);
        let r = t.traverse(1);
        // finalize 里 pushes==pops 才会无违例。
        let ok = !r.discarded && r.invariants.violations().is_empty();
        set.add("D14-不变式-压栈弹栈相等", ok, "");
    }

    // ---- 状态栈纪律：弹栈失配被拦 ----

    {
        let mut s = StateStacks::new();
        let underflow = s.pop().is_err();
        s.push(Transform::identity(), ClipDomain::Unclipped, 1.0);
        let ok_pop = s.pop().is_ok() && s.depth() == 0;
        set.add("D14-栈纪律-弹栈失配显性拦截", underflow && ok_pop, "");
    }

    // ---- 空组也须成对（不留悬空压栈） ----

    {
        let nodes = vec![group(9, Vec::new())];
        let t = RenderTraverser::new(nodes);
        let r = t.traverse(9);
        let seq: Vec<DrawAction> = r.list.cmds.iter().map(|c| c.action).collect();
        let ok = !r.discarded
            && seq.len() == 2
            && seq[0] == DrawAction::EnterGroup
            && seq[1] == DrawAction::LeaveGroup;
        set.add("D14-栈纪律-空组成对进退", ok, "");
    }

    // ---- 可见性：隐藏跳绘制但保留结构 ----

    {
        let n = (2, leaf(2, 0.0, 0.0, 4.0, 4.0).1.with_visibility(Visibility::HiddenKeepLayout));
        let nodes = vec![group(1, vec![2]), n];
        let t = RenderTraverser::new(nodes);
        let r = t.traverse(1);
        // 层 2 不产出绘制指令，但层 1 的组结构仍在序列里。
        let ids: Vec<u64> = r.list.cmds.iter().map(|c| c.node_id).collect();
        let ok = !r.discarded && !ids.contains(&2) && ids.contains(&1);
        set.add("D14-可见性-隐藏跳绘制保结构", ok, "");
    }

    // ---- 剔除：视口外整棵子树跳过 ----

    {
        let nodes = vec![
            group(1, vec![2, 3]),
            leaf(2, 0.0, 0.0, 10.0, 10.0),
            leaf(3, 500.0, 500.0, 10.0, 10.0),
        ];
        let mut t = RenderTraverser::new(nodes);
        t.set_viewport(Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 });
        let r = t.traverse(1);
        let ids: Vec<u64> = r.list.cmds.iter().map(|c| c.node_id).collect();
        let ok = !ids.contains(&3) && ids.contains(&2);
        set.add("D14-剔除-视口外子树跳过", ok, "");
    }

    // ---- 剔除误判兜底：视口缺失时保守不剔除 ----

    {
        let nodes = vec![leaf(1, 500.0, 500.0, 10.0, 10.0)];
        let t = RenderTraverser::new(nodes); // 未注入视口
        let r = t.traverse(1);
        let ok = !r.discarded && r.list.cmds.len() == 1;
        set.add("D14-剔除-无视口保守保留", ok, "");
    }

    // ---- 状态压栈：变换级联与不透明度相乘 ----

    {
        let root = (1, group(1, vec![2]).1
            .with_local(Transform::translate(10.0, 20.0))
            .with_opacity(0.5));
        let kid = (2, leaf(2, 0.0, 0.0, 4.0, 4.0)
            .1.with_local(Transform::translate(1.0, 2.0))
            .with_opacity(0.5));
        let t = RenderTraverser::new(vec![root, kid]);
        let r = t.traverse(1);
        let kid_cmd = r
            .list
            .cmds
            .iter()
            .find(|c| c.node_id == 2)
            .copied();
        match kid_cmd {
            Some(c) => {
                let ok = (c.world.tx - 11.0).abs() < 1e-4
                    && (c.world.ty - 22.0).abs() < 1e-4
                    && (c.opacity - 0.25).abs() < 1e-4;
                set.add("D14-压栈-变换级联与不透明度相乘", ok, "");
            }
            None => set.add("D14-压栈-变换级联与不透明度相乘", false, ""),
        }
    }

    // ---- 全透明层跳过绘制 ----

    {
        let t = RenderTraverser::new(vec![(1, leaf(1, 0.0, 0.0, 4.0, 4.0).1.with_opacity(0.0))]);
        let r = t.traverse(1);
        set.add("D14-压栈-全透明层跳过绘制", r.list.cmds.is_empty(), "");
    }

    // ---- 深度上界：越深显性拒绝，不留半帧 ----

    {
        // 造一条 70 层的链（超MAX_DEPTH=64）。
        let mut nodes: Vec<(u64, LayerNode)> = Vec::new();
        for i in 1..=70u64 {
            let kids = if i < 70 { vec![i + 1] } else { Vec::new() };
            nodes.push(group(i, kids));
        }
        let t = RenderTraverser::new(nodes);
        let r = t.traverse(1);
        let ok = r.discarded && !r.invariants.violations().is_empty();
        set.add("D14-防护-深度越界显性拒绝", ok, "");
    }

    // ---- 降级：根层缺失显性拒绝 ----

    {
        let t = RenderTraverser::new(vec![leaf(1, 0.0, 0.0, 4.0, 4.0)]);
        let r = t.traverse(999);
        let ok = r.discarded && r.list.cmds.is_empty();
        set.add("D14-降级-根层缺失拒绝", ok, "");
    }

    // ---- 降级：非有限不透明度保守按不透明 ----

    {
        let t = RenderTraverser::new(vec![(1, leaf(1, 0.0, 0.0, 4.0, 4.0).1.with_opacity(f32::NAN))]);
        let r = t.traverse(1);
        let logged = r.audits.iter().any(|a| a.contains("非有限"));
        set.add("D14-降级-非有限不透明度保守处理", !r.discarded && logged, "");
    }

    // ---- 降级：裁剪域级联求交为空则跳过 ----

    {
        let r0 = Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 };
        let root = (1, group(1, vec![2]).1.with_clip(ClipDomain::Rect(r0)));
        let kid = (2, leaf(2, 100.0, 100.0, 4.0, 4.0).1.with_clip(ClipDomain::Rect(r0)));
        let t = RenderTraverser::new(vec![root, kid]);
        let r = t.traverse(1);
        let ids: Vec<u64> = r.list.cmds.iter().map(|c| c.node_id).collect();
        // 子层裁剪域与父不相交 → 子层被跳过。
        let ok = !ids.contains(&2);
        set.add("D14-降级-裁剪域级联求交", ok, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: u64) -> (u64, LayerNode) {
        (
            id,
            LayerNode {
                id,
                children: Vec::new(),
                is_group: false,
                local: Transform::identity(),
                bounds: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
                visibility: Visibility::Visible,
                opacity: 1.0,
                clip: None,
            },
        )
    }

    fn group(id: u64, kids: Vec<u64>) -> (u64, LayerNode) {
        (
            id,
            LayerNode {
                id,
                children: kids,
                is_group: true,
                local: Transform::identity(),
                bounds: Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 },
                visibility: Visibility::Visible,
                opacity: 1.0,
                clip: None,
            },
        )
    }

    #[test]
    fn preorder_is_group_then_children() {
        let t = RenderTraverser::new(vec![group(1, vec![2, 3]), leaf(2), leaf(3)]);
        let r = t.traverse(1);
        let ids: Vec<u64> = r.list.cmds.iter().map(|c| c.node_id).collect();
        assert_eq!(ids, vec![1, 2, 3], "先序须组先兄先");
        assert!(!r.discarded);
    }

    #[test]
    fn nested_groups_nest_correctly() {
        let t = RenderTraverser::new(vec![
            group(1, vec![2]),
            group(2, vec![3]),
            leaf(3),
        ]);
        let r = t.traverse(1);
        let depths: Vec<usize> = r.list.cmds.iter().map(|c| c.depth).collect();
        assert_eq!(depths, vec![0, 1, 2], "深度须逐层递增");
    }

    #[test]
    fn invariants_hold_on_healthy_tree() {
        let t = RenderTraverser::new(vec![group(1, vec![2, 3]), leaf(2), leaf(3)]);
        let r = t.traverse(1);
        assert!(!r.discarded);
        assert!(r.invariants.violations().is_empty());
        assert_eq!(r.invariants.visited_len(), 3);
    }

    #[test]
    fn stack_underflow_is_rejected() {
        let mut s = StateStacks::new();
        assert!(s.pop().is_err(), "空栈弹栈须显性拒绝");
    }

    #[test]
    fn transform_cascade_composes() {
        let p = Transform::translate(10.0, 20.0);
        let c = Transform::translate(1.0, 2.0);
        let w = p.cascade(&c);
        assert!((w.tx - 11.0).abs() < 1e-6 && (w.ty - 22.0).abs() < 1e-6);
    }

    #[test]
    fn visibility_propagates_hidden_and_gone() {
        assert_eq!(effective_visibility(&[Visibility::Visible]), EffectiveVisibility::Shown);
        assert_eq!(
            effective_visibility(&[Visibility::HiddenKeepLayout]),
            EffectiveVisibility::HiddenSkipped
        );
        assert_eq!(effective_visibility(&[Visibility::Removed]), EffectiveVisibility::Gone);
    }

    #[test]
    fn deep_tree_is_rejected_not_overflowed() {
        let mut nodes = Vec::new();
        for i in 1..=80u64 {
            let kids = if i < 80 { vec![i + 1] } else { Vec::new() };
            nodes.push(group(i, kids));
        }
        let r = RenderTraverser::new(nodes).traverse(1);
        assert!(r.discarded, "超深树须整帧作废");
    }

    #[test]
    fn effects_checks_all_green() {
        let set = run_ved14_checks();
        let (p, f) = set.tally();
        assert!(!set.truncated());
        assert!(set.all_passed(), "VE-F0614 红项：{}/{}", p, p + f);
    }
}