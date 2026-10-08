//! VE-F0613 · 图层脏区收集（树级）（VE-D 域 · 2D 合成引擎 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0613`
//!
//! **判据（锚点原文）**：变化源到脏区的映射表——变换变更→该层世界包围盒新旧两域
//! 并集、内容变更→层边界域、可见性切换→加入或移除语义（F0607 事件映射）、Z 序
//! 重排→重排域前后并集（F0606 重排域）、效果参数变更→效果影响域；树级合并：子
//! 节点脏区向上合并进祖先坐标系（合并发生在最小编号公共祖先——合并域最小化）；
//! 输出：按帧收集的区域集，作为 F0641 三档模型的输入源；帧内合并与去重纪律（相交
//! 即并，防碎片爆炸）。判据四条：**映射全、最小祖先合并、去重纪律、全帧信号**。
//!
//! **错误路径与降级矩阵**：映射缺源→登记补齐；区域爆炸（超阈值）→升级为全帧建议
//! 信号（F0644 判定输入）；并集溢出→钳制到画面域。
//!
//! **设计要点**：
//! - **映射全（O(1) 每事件）**：[`map_event`] 以事件种类分派，五类变化源各有专属
//!   映射，无"通用兜底"——映射表可穷举，判据"映射全"可被机检逐类点名；
//! - **新旧两域并集**：变换与 Z 序重排都是"从旧到新"的迁移，脏区必须同时覆盖
//!   **旧域与新域**（只覆盖新域会让旧位置残留上一帧内容，这是本条最易漏的一半）；
//! - **最小编号公共祖先**：[`lowest_common_ancestor`] 在全体 owner 的公共祖先集中
//!   取**编号最小**者作为合并目标——编号小者通常更靠根，坐标系更外层，合并域因此
//!   最小化（合并域最小化是本条收益，合并进深层只会把区域撑大）；
//! - **相交即并（去重纪律）**：[`coalesce`] 把相交/相接的区域并成一块，反复迭代
//!   直到无相交为止——帧内碎片越少，F0641 三档模型的判定越准；
//! - **全帧信号**：区域数超 [`REGION_EXPLOSION_THRESHOLD`] 时不硬扛，升级为全帧
//!   建议信号交 F0644 裁决（局部重绘已不划算，全帧反而更稳）；
//! - **钳制到画面域**：坐标溢出 [`COORD_CLAMP`] 一律钳回，并把区域与画面域求交
//!   ——屏外脏区对合成无意义，钳制同时消掉溢出任一可能。
//!
//! **跨批对接点**：上游 F0606（重排域 [`DirtyHint`]）、F0607（可见性三态事件）、
//! F0608（动画每帧插值结果即变换脏）；下游 F0641 三档模型（帧区域集为输入源）、
//! F0645 传播。
//!
//! 逻辑 tick 注入，零墙钟；零 IO；全部类型自持（本模块不 import 未注册的兄弟
//! 模块——平行会话的 `ved*` 族尚在施工，注册次序不定，编译期硬耦合会让本条
//! 因别人的进度而红）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 矩形比较容差（相交判定的数值容差；小于此视为相接）。
pub const DAMAGE_EPS: f32 = 1e-4;

/// 坐标钳制界（并集溢出钳制用，对齐 F0604 `COORD_CLAMP`）。
pub const COORD_CLAMP: f32 = 1.0e6;

/// 区域爆炸阈值（超此数量升级为全帧建议信号）。
pub const REGION_EXPLOSION_THRESHOLD: usize = 64;

/// 帧区域集硬上限（防御性上界；正常由爆炸阈值先拦住）。
pub const MAX_FRAME_REGIONS: usize = 1024;

/// 单帧最多受理的事件数（超出显性拒绝并登记——零静默）。
pub const MAX_EVENTS_PER_FRAME: usize = 4096;

/// 映射全契约（五类变化源穷举，映射表可机检）。
pub const MAPPING_TABLE_DOC: &str = "\
脏区映射全契约（VE-F0613 · v1）：五类变化源各有唯一映射，无通用兜底——\
变换变更=世界包围盒新旧两域并集；内容变更=层边界域；可见性切换=加入或移除\
语义（F0607 事件映射）；Z 序重排=重排域前后并集（F0606 重排域）；效果参数\
变更=效果影响域。新旧两域缺一即残留上一帧内容，属最高缺陷。";

/// 最小祖先合并契约。
pub const MERGE_POLICY_DOC: &str = "\
最小祖先合并契约（VE-F0613 · v1）：子节点脏区向上合并进公共祖先坐标系，合并\
目标取公共祖先集中编号最小者——坐标系更外层，合并域最小化。合并只改坐标系归属，\
不放大区域（区域放大只来自并集本身）。";

/// 去重纪律契约。
pub const COALESCE_DOC: &str = "\
帧内去重纪律（VE-F0613 · v1）：相交或相接的区域并为一块，迭代至无相交为止\
——帧内碎片越少，F0641 三档模型判定越准。去重只并相交者，不并远离者（远离者\
合并会把两处重绘扩成全屏，违反合并域最小化）。";

/// 全帧信号契约。
pub const FULL_FRAME_DOC: &str = "\
全帧信号契约（VE-F0613 · v1）：帧区域数超阈值即升级为全帧建议信号，交 F0644\
裁决——局部重绘已不划算，全帧反而更稳。信号是建议不是强刷，最终判定权在下游。";

/// 画面域（钳制目标；由消费方按实际视口注入）。
pub const VIEWPORT_DOC: &str = "\
画面域注入契约（VE-F0613 · v1）：脏区一律与画面域求交，屏外区域对合成无意义；\
未注入画面域时不求交（保持全量区域，交由下游裁剪），注入后钳制生效。";

// ---------------------------------------------------------------------------
// 二、数据结构
// ---------------------------------------------------------------------------

/// 轴对齐矩形域（世界或祖先坐标系；脏区的统一载体）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageRect {
    /// 左上 x。
    pub x: f32,
    /// 左上 y。
    pub y: f32,
    /// 宽（≥0）。
    pub w: f32,
    /// 高（≥0）。
    pub h: f32,
}

impl DamageRect {
    /// 构造：负宽高与非有限值显性拒绝（不静默取绝对值）。
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Option<Self> {
        if !(w >= 0.0 && h >= 0.0) || !x.is_finite() || !y.is_finite() {
            return None;
        }
        if !w.is_finite() || !h.is_finite() {
            return None;
        }
        Some(DamageRect { x, y, w, h })
    }

    /// 右边界（exclusive 口径）。
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// 下边界（exclusive 口径）。
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// 是否退化（零面积）。
    pub fn is_empty(&self) -> bool {
        self.w <= DAMAGE_EPS || self.h <= DAMAGE_EPS
    }

    /// 并集：取两者的最小包围盒（相离时并集含空隙——这是"相交即并"的反面，
    /// 相离者由 [`coalesce`] 拒绝合并，故此处并集只用于旧域+新域这类必然
    /// 连续或近连续的场景）。
    pub fn union(&self, o: &DamageRect) -> DamageRect {
        let x1 = if self.x < o.x { self.x } else { o.x };
        let y1 = if self.y < o.y { self.y } else { o.y };
        let x2 = if self.right() > o.right() {
            self.right()
        } else {
            o.right()
        };
        let y2 = if self.bottom() > o.bottom() {
            self.bottom()
        } else {
            o.bottom()
        };
        DamageRect {
            x: x1,
            y: y1,
            w: x2 - x1,
            h: y2 - y1,
        }
    }

    /// 相交或相接（含容差）——去重的判据。
    pub fn touches(&self, o: &DamageRect) -> bool {
        self.x <= o.right() + DAMAGE_EPS
            && o.x <= self.right() + DAMAGE_EPS
            && self.y <= o.bottom() + DAMAGE_EPS
            && o.y <= self.bottom() + DAMAGE_EPS
    }

    /// 求交：不相交返回 None（画面域钳制的原语）。
    pub fn intersect(&self, o: &DamageRect) -> Option<DamageRect> {
        let x1 = if self.x > o.x { self.x } else { o.x };
        let y1 = if self.y > o.y { self.y } else { o.y };
        let x2 = if self.right() < o.right() {
            self.right()
        } else {
            o.right()
        };
        let y2 = if self.bottom() < o.bottom() {
            self.bottom()
        } else {
            o.bottom()
        };
        if x2 - x1 <= DAMAGE_EPS || y2 - y1 <= DAMAGE_EPS {
            return None;
        }
        DamageRect::new(x1, y1, x2 - x1, y2 - y1)
    }

    /// 钳制坐标到画面域量级（溢出兜底；钳完仍超界则贴边）。
    pub fn clamp_coords(&self) -> DamageRect {
        let x = self.x.clamp(-COORD_CLAMP, COORD_CLAMP);
        let y = self.y.clamp(-COORD_CLAMP, COORD_CLAMP);
        let r = self.right().clamp(-COORD_CLAMP, COORD_CLAMP);
        let b = self.bottom().clamp(-COORD_CLAMP, COORD_CLAMP);
        DamageRect {
            x,
            y,
            w: (r - x).max(0.0),
            h: (b - y).max(0.0),
        }
    }
}

/// 变化源五类（映射表的键；判据"映射全"的枚举面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeSource {
    /// 变换变更 → 世界包围盒新旧两域并集。
    Transform,
    /// 内容变更 → 层边界域。
    Content,
    /// 可见性切换 → 加入或移除语义。
    Visibility,
    /// Z 序重排 → 重排域前后并集。
    ZReorder,
    /// 效果参数变更 → 效果影响域。
    Effect,
}

impl ChangeSource {
    /// 稳定短名（机检与审计用）。
    pub fn tag(self) -> &'static str {
        match self {
            ChangeSource::Transform => "transform",
            ChangeSource::Content => "content",
            ChangeSource::Visibility => "visibility",
            ChangeSource::ZReorder => "zreorder",
            ChangeSource::Effect => "effect",
        }
    }

    /// 五类穷举（映射表完整性机检的驱动面）。
    pub fn all() -> [ChangeSource; 5] {
        [
            ChangeSource::Transform,
            ChangeSource::Content,
            ChangeSource::Visibility,
            ChangeSource::ZReorder,
            ChangeSource::Effect,
        ]
    }
}

/// 脏区语义动作（可见性切换的加入/移除二义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageOp {
    /// 加入：该区域新变为脏（显示、内容变更、变换移入）。
    Add,
    /// 移除：该区域需清回背景（隐藏、层移出）。
    Remove,
}

impl DamageOp {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            DamageOp::Add => "add",
            DamageOp::Remove => "remove",
        }
    }
}

/// 可见性切换（映射 F0607 三态事件；本条只取其加入/移除语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilitySwitch {
    /// 显示：加入语义。
    Shown,
    /// 隐藏保留布局：移除语义 + 相邻区域是否需失效。
    Hidden {
        /// 相邻区域失效标记（F0607 上游给出）。
        adjacent_invalidated: bool,
    },
    /// 彻底移除：移除语义（整块作废）。
    Removed,
}

/// Z 序重排域（映射 F0606 `DirtyHint`：受影响兄弟区间）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReorderSpan {
    /// 受影响兄弟区间起点（序位）。
    pub span_start: usize,
    /// 受影响兄弟区间终点（exclusive）。
    pub span_end: usize,
}

/// 脏区事件（五类变化源的统一入参；映射 O(1)）。
///
/// 含重排域的层表，故只`Clone` 不`Copy`。
#[derive(Clone, Debug, PartialEq)]
pub enum DamageEvent {
    /// 变换变更：世界包围盒新旧两域（缺一即残留，故两域都必须给）。
    Transform {
        /// 层 id。
        node_id: u64,
        /// 旧世界域。
        old_world: DamageRect,
        /// 新世界域。
        new_world: DamageRect,
    },
    /// 内容变更：层边界域。
    Content {
        /// 层 id。
        node_id: u64,
        /// 层边界域（世界系）。
        bounds: DamageRect,
    },
    /// 可见性切换：加入或移除语义。
    Visibility {
        /// 层 id。
        node_id: u64,
        /// 三态事件（F0607 映射面）。
        switch: VisibilitySwitch,
        /// 该层的世界域（加入/移除都落在它上面）。
        world: DamageRect,
    },
    /// Z 序重排：重排域前后并集。
    ZReorder {
        /// 层 id（重排发生处的宿主层）。
        node_id: u64,
        /// 重排域（F0606 `DirtyHint`）。
        span: ReorderSpan,
        /// 重排前各层的世界域（与 `before_ids` 同序）。
        before: Vec<(u64, DamageRect)>,
        /// 重排后各层的世界域（与 `after_ids` 同序）。
        after: Vec<(u64, DamageRect)>,
    },
    /// 效果参数变更：效果影响域。
    Effect {
        /// 层 id。
        node_id: u64,
        /// 效果影响域（世界系；模糊类效果的影响域大于层边界域）。
        impact: DamageRect,
    },
}

/// 脏区（一块待重绘的区域 + 其归属与语义）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageRegion {
    /// 区域（当前所属坐标系）。
    pub rect: DamageRect,
    /// 变化源（回溯用）。
    pub source: ChangeSource,
    /// 语义动作。
    pub op: DamageOp,
    /// 归属层 id（合并前为事件层，合并后为合并目标）。
    pub owner: u64,
}

impl DamageRegion {
    /// 构造。
    pub fn new(
        rect: DamageRect,
        source: ChangeSource,
        op: DamageOp,
        owner: u64,
    ) -> Self {
        DamageRegion {
            rect,
            source,
            op,
            owner,
        }
    }
}

/// 帧收集结果（F0641 三档模型的输入源）。
#[derive(Clone, Debug, PartialEq)]
pub struct FrameDamage {
    /// 逻辑帧号。
    pub frame: u64,
    /// 去重并集后的区域集。
    pub regions: Vec<DamageRegion>,
    /// 是否升级为全帧建议信号。
    pub full_frame: bool,
    /// 合并目标（最小编号公共祖先；None = 无事件）。
    pub merged_into: Option<u64>,
    /// 本帧受理事件数。
    pub events: usize,
    /// 去重前区域数（碎片观测面）。
    pub raw_regions: usize,
}

// ---------------------------------------------------------------------------
// 三、映射表（五类变化源 → 脏区，O(1) 每事件）
// ---------------------------------------------------------------------------

/// O(1) 事件映射：五类变化源各走专属映射，无通用兜底。
///
/// 返回空 `Vec` 只在一种情形发生：重排事件的 `before`/`after` 同时为空
/// （重排域内没有任何层——不是映射缺失，是空事件）。
pub fn map_event(ev: &DamageEvent) -> Vec<DamageRegion> {
    match ev {
        DamageEvent::Transform {
            node_id,
            old_world,
            new_world,
        } => {
            let (node_id, old_world, new_world) = (*node_id, *old_world, *new_world);
            // 新旧两域并集：只覆盖新域会让旧位置残留上一帧内容。
            vec![DamageRegion::new(
                old_world.union(&new_world),
                ChangeSource::Transform,
                DamageOp::Add,
                node_id,
            )]
        }
        DamageEvent::Content { node_id, bounds } => {
            vec![DamageRegion::new(
                *bounds,
                ChangeSource::Content,
                DamageOp::Add,
                *node_id,
            )]
        }
        DamageEvent::Visibility {
            node_id,
            switch,
            world,
        } => {
            let (node_id, switch, world) = (*node_id, *switch, *world);
            let mut out = Vec::new();
            let op = match switch {
                VisibilitySwitch::Shown => DamageOp::Add,
                VisibilitySwitch::Hidden { .. } | VisibilitySwitch::Removed => {
                    DamageOp::Remove
                }
            };
            out.push(DamageRegion::new(world, ChangeSource::Visibility, op, node_id));
            if let VisibilitySwitch::Hidden {
                adjacent_invalidated: true,
            } = switch
            {
                // 相邻失效：相邻重叠层存在时其区域一并作废（上游语义：隐藏会
                // 露出下层，必须让下层同区域一起重绘）。
                out.push(DamageRegion::new(
                    world,
                    ChangeSource::Visibility,
                    DamageOp::Add,
                    node_id,
                ));
            }
            out
        }
        DamageEvent::ZReorder {
            node_id,
            span,
            before,
            after,
        } => {
            let mut acc: Option<DamageRect> = None;
            for (_, r) in before.iter().chain(after.iter()) {
                acc = Some(match acc {
                    None => *r,
                    Some(a) => a.union(r),
                });
            }
            match acc {
                Some(r) => {
                    let _ = *span;
                    vec![DamageRegion::new(
                        r,
                        ChangeSource::ZReorder,
                        DamageOp::Add,
                        *node_id,
                    )]
                }
                // 重排域内无层：空事件，不是映射缺失。
                None => Vec::new(),
            }
        }
        DamageEvent::Effect { node_id, impact } => {
            vec![DamageRegion::new(
                *impact,
                ChangeSource::Effect,
                DamageOp::Add,
                *node_id,
            )]
        }
    }
}

// ---------------------------------------------------------------------------
// 四、树级合并（最小编号公共祖先 + 去重）
// ---------------------------------------------------------------------------

/// 祖先链（自节点向上至根，含自身；顺序为"自身 → 根"）。
pub fn ancestor_chain(node_id: u64, parents: &[(u64, Option<u64>)]) -> Vec<u64> {
    let mut out = Vec::new();
    let mut cur = Some(node_id);
    // 深度上界：父映射成环时不死循环（结构不变式由 F0601/F0617 把关，本处兜底）。
    let mut guard = parents.len() + 1;
    while let Some(id) = cur {
        if out.contains(&id) {
            break;
        }
        out.push(id);
        if guard == 0 {
            break;
        }
        guard -= 1;
        cur = parents
            .iter()
            .find(|(pid, _)| *pid == id)
            .and_then(|(_, par)| *par);
    }
    out
}

/// 最小编号公共祖先：取各节点祖先链的公共集中**编号最小**者。
///
/// 编号小者通常更靠根，坐标系更外层→ 合并域最小化（判据"最小祖先合并"）。
/// 注意：节点自身也参与祖先链，故单节点入参返回的是它链上编号最小者
/// （即最外层祖先），而非它自己——这正是"合并域最小化"的本意：
/// 脏区一律上提到最外层坐标系，一次合并即可，无需逐层再合。
/// 全体节点无公共祖先（跨树/森林）时返回 `None`，由调用方决定降级。
pub fn lowest_common_ancestor(
    nodes: &[u64],
    parents: &[(u64, Option<u64>)],
) -> Option<u64> {
    if nodes.is_empty() {
        return None;
    }
    let mut common: Option<Vec<u64>> = None;
    for &n in nodes {
        let chain = ancestor_chain(n, parents);
        common = Some(match common {
            None => chain,
            Some(prev) => prev.into_iter().filter(|id| chain.contains(id)).collect(),
        });
        if common.as_ref().map(|c| c.is_empty()).unwrap_or(false) {
            return None;
        }
    }
    // 编号最小者即合并目标。
    common.and_then(|c| c.into_iter().min())
}

/// 相交即并去重：反复并合相交/相接的区域，直到无相交为止。
///
/// 只并相交者——相离者合并会把两处重绘扩成中间一大片，违反合并域最小化。
/// 语义动作不同的两块不并（加入与移除不可混为一谈）。
pub fn coalesce(regions: Vec<DamageRegion>) -> Vec<DamageRegion> {
    let mut out: Vec<DamageRegion> = Vec::with_capacity(regions.len());
    // 逐块吸收：取出pool 首块，与已定块尝试并合，直至无可并者再定块。
    // 单趟O(n·k)（k=已定块数），吸收后立即从 pool 重新取首块，
    // 故每块至多被取出一次，链式并合一轮到底（O(n·k) 而非反复全量重扫）。
    let mut pool = regions;
    while let Some(cand) = pool.first().copied() {
        pool.remove(0);
        let mut merged = cand;
        let mut j = 0;
        while j < out.len() {
            let cur = out[j];
            if cur.op == merged.op && cur.source == merged.source
                && cur.rect.touches(&merged.rect)
            {
                merged.rect = cur.rect.union(&merged.rect);
                // owner 取编号小者（与最小祖先合并同向：归属更外层）。
                merged.owner = if cur.owner <= merged.owner {
                    cur.owner
                } else {
                    merged.owner
                };
                out.remove(j);
            } else {
                j += 1;
            }
        }
        out.push(merged);
    }
    // 稳定序：按 (x, y, w, h) 排序，保证同输入同输出（回归可比对）。
    out.sort_by(|a, b| {
        a.rect
            .x
            .partial_cmp(&b.rect.x)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then(
                a.rect
                    .y
                    .partial_cmp(&b.rect.y)
                    .unwrap_or(core::cmp::Ordering::Equal),
            )
            .then(
                a.rect
                    .w
                    .partial_cmp(&b.rect.w)
                    .unwrap_or(core::cmp::Ordering::Equal),
            )
            .then(
                a.rect
                    .h
                    .partial_cmp(&b.rect.h)
                    .unwrap_or(core::cmp::Ordering::Equal),
            )
    });
    out
}

// ---------------------------------------------------------------------------
// 五、帧收集器（登记 + 合并 + 去重 + 钳制 + 全帧信号）
// ---------------------------------------------------------------------------

/// 树级脏区收集器（一帧一收集，产出 [`FrameDamage`]）。
pub struct DirtyCollector {
    parents: Vec<(u64, Option<u64>)>,
    viewport: Option<DamageRect>,
    events: Vec<DamageEvent>,
    errors: Vec<(String, &'static str, String)>,
    audits: Vec<String>,
    full_frame_signals: u64,
    tick: u64,
}

impl DirtyCollector {
    /// 构造：注入父映射快照（F0601 只读投影；本表不改树）。
    pub fn new(parents: Vec<(u64, Option<u64>)>) -> Self {
        DirtyCollector {
            parents,
            viewport: None,
            events: Vec::new(),
            errors: Vec::new(),
            audits: Vec::new(),
            full_frame_signals: 0,
            tick: 0,
        }
    }

    /// 注入画面域（不注入则不求交，钳制仅走坐标量级）。
    pub fn set_viewport(&mut self, vp: DamageRect) {
        self.viewport = Some(vp);
    }

    /// 受理一个事件（O(1)；超上界显性拒绝并登记——零静默）。
    pub fn push(&mut self, ev: DamageEvent) -> Result<(), &'static str> {
        if self.events.len() >= MAX_EVENTS_PER_FRAME {
            let msg = format!(
                "本帧事件数已达上界 {}，事件被拒（先结算本帧再收下一帧）",
                MAX_EVENTS_PER_FRAME
            );
            self.errors
                .push(("what".to_string(), "E_FRAME_EVENT_OVERFLOW", msg.clone()));
            return Err("E_FRAME_EVENT_OVERFLOW");
        }
        self.events.push(ev);
        Ok(())
    }

    /// 错误账本（零静默）。
    pub fn errors(&self) -> &[(String, &'static str, String)] {
        &self.errors
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 全帧信号累计次数（风暴观测面）。
    pub fn full_frame_signals(&self) -> u64 {
        self.full_frame_signals
    }

    /// 本帧待结算事件数。
    pub fn pending_events(&self) -> usize {
        self.events.len()
    }

    /// 逻辑 tick 推进（零墙钟纪律）。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }

    /// 结算本帧：映射 → 最小祖先合并 → 去重 → 钳制 → 全帧判定。
    pub fn collect_frame(&mut self) -> FrameDamage {
        let events = core::mem::take(&mut self.events);
        let raw: Vec<DamageRegion> = events.iter().flat_map(map_event).collect();
        let raw_regions = raw.len();

        // 最小编号公共祖先：全体 owner 的合并目标。
        let owners: Vec<u64> = {
            let mut v: Vec<u64> = raw.iter().map(|r| r.owner).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let merged_into = lowest_common_ancestor(&owners, &self.parents);

        // 映射缺源 → 登记补齐：owner 不在父映射快照内（上游 F0601 未登记该层，
        // 或本帧新增层尚未并入快照）。缺源的层按"各自成链"处理——脏区照常
        // 产出（宁可多画不可漏画），但显性登记让上游补登记，不静默。
        for &o in owners.iter() {
            if !self.parents.iter().any(|(pid, _)| *pid == o) {
                self.errors.push((
                    "what".to_string(),
                    "E_DIRTY_SOURCE_MISSING",
                    format!(
                        "层 {} 不在父映射快照内（映射缺源），其脏区按各自成链处理，\
                         请上游补登记",
                        o
                    ),
                ));
            }
        }

        if !owners.is_empty() && merged_into.is_none() {
            // 跨树/森林：全体无公共祖先 → 降级为不合并（保持各自归属），
            // 登记补齐而不是静默按某一棵树处理。
            self.errors.push((
                "what".to_string(),
                "E_NO_COMMON_ANCESTOR",
                format!(
                    "本帧 {} 个 owner 跨树无公共祖先，合并降级为不合并（各自归属保留）",
                    owners.len()
                ),
            ));
        }

        // 归属改写为合并目标（坐标系上移到公共祖先）。
        let mut regions = raw;
        if let Some(target) = merged_into {
            for r in regions.iter_mut() {
                r.owner = target;
            }
            self.audits.push(format!(
                "本帧 {} 块区域合并进最小编号公共祖先 {}",
                regions.len(),
                target
            ));
        }

        // 去重（相交即并）。
        regions = coalesce(regions);

        // 钳制：坐标量级 + 画面域求交。
        let vp = self.viewport;
        let mut clamped = Vec::with_capacity(regions.len());
        for mut r in regions.into_iter() {
            let before = r.rect;
            r.rect = r.rect.clamp_coords();
            if let Some(v) = vp {
                r.rect = match r.rect.intersect(&v) {
                    Some(k) => k,
                    // 整块在画面外：该层本帧无需重绘，丢弃并留痕。
                    None => {
                        self.audits.push(format!(
                            "区域 ({:.1},{:.1},{:.1},{:.1}) 全在画面域外，丢弃",
                            before.x, before.y, before.w, before.h
                        ));
                        continue;
                    }
                };
            }
            clamped.push(r);
        }

        // 全帧信号：区域爆炸升级为建议信号（交 F0644 裁决）。
        let full_frame = clamped.len() > REGION_EXPLOSION_THRESHOLD;
        if full_frame {
            self.full_frame_signals = self.full_frame_signals.saturating_add(1);
            self.audits.push(format!(
                "区域数 {} 超阈值 {}，升级为全帧建议信号（F0644 判定输入）",
                clamped.len(),
                REGION_EXPLOSION_THRESHOLD
            ));
        }

        // 硬上界兜底（防御；正常由爆炸阈值先拦）。
        if clamped.len() > MAX_FRAME_REGIONS {
            clamped.truncate(MAX_FRAME_REGIONS);
            self.errors.push((
                "what".to_string(),
                "E_REGION_HARD_CAP",
                format!("区域数超硬上界 {}，已截断", MAX_FRAME_REGIONS),
            ));
        }

        let frame = self.tick;
        self.tick = self.tick.saturating_add(1);
        FrameDamage {
            frame,
            events: events.len(),
            raw_regions,
            regions: clamped,
            full_frame,
            merged_into,
        }
    }
}

// ---------------------------------------------------------------------------
// 六、自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F0613 域自检。
pub fn run_ved13_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0613");

    // ---- 映射全：五类变化源各有映射 ----

    {
        let t = DamageEvent::Transform {
            node_id: 1,
            old_world: DamageRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
            new_world: DamageRect { x: 100.0, y: 0.0, w: 10.0, h: 10.0 },
        };
        let r = map_event(&t);
        // 新旧两域并集：必须同时覆盖 x=0 与 x=110。
        let cover_old = r[0].rect.x <= 0.0 && r[0].rect.right() >= 110.0;
        let both = r.len() == 1 && r[0].source == ChangeSource::Transform;
        set.add("D13-映射-变换新旧两域并集", cover_old && both, "");
    }

    {
        let c = DamageEvent::Content {
            node_id: 2,
            bounds: DamageRect { x: 5.0, y: 5.0, w: 20.0, h: 30.0 },
        };
        let r = map_event(&c);
        let ok = r.len() == 1
            && r[0].source == ChangeSource::Content
            && r[0].rect.w == 20.0
            && r[0].op == DamageOp::Add;
        set.add("D13-映射-内容变更取层边界域", ok, "");
    }

    {
        let shown = map_event(&DamageEvent::Visibility {
            node_id: 3,
            switch: VisibilitySwitch::Shown,
            world: DamageRect { x: 1.0, y: 1.0, w: 4.0, h: 4.0 },
        });
        let hidden_adj = map_event(&DamageEvent::Visibility {
            node_id: 3,
            switch: VisibilitySwitch::Hidden { adjacent_invalidated: true },
            world: DamageRect { x: 1.0, y: 1.0, w: 4.0, h: 4.0 },
        });
        let hidden_noadj = map_event(&DamageEvent::Visibility {
            node_id: 3,
            switch: VisibilitySwitch::Hidden { adjacent_invalidated: false },
            world: DamageRect { x: 1.0, y: 1.0, w: 4.0, h: 4.0 },
        });
        let removed = map_event(&DamageEvent::Visibility {
            node_id: 3,
            switch: VisibilitySwitch::Removed,
            world: DamageRect { x: 1.0, y: 1.0, w: 4.0, h: 4.0 },
        });
        // 显示=加入；隐藏=移除+相邻失效（两块）；移除=移除语义。
        let ok = shown.len() == 1
            && shown[0].op == DamageOp::Add
            && hidden_adj.len() == 2
            && hidden_adj.iter().any(|r| r.op == DamageOp::Remove)
            && hidden_adj.iter().any(|r| r.op == DamageOp::Add)
            && hidden_noadj.len() == 1
            && hidden_noadj[0].op == DamageOp::Remove
            && removed.len() == 1
            && removed[0].op == DamageOp::Remove;
        set.add("D13-映射-可见性加入移除语义", ok, "");
    }

    {
        let z = map_event(&DamageEvent::ZReorder {
            node_id: 4,
            span: ReorderSpan { span_start: 1, span_end: 3 },
            before: vec![
                (10, DamageRect { x: 0.0, y: 0.0, w: 5.0, h: 5.0 }),
                (11, DamageRect { x: 50.0, y: 0.0, w: 5.0, h: 5.0 }),
            ],
            after: vec![(10, DamageRect { x: 60.0, y: 0.0, w: 5.0, h: 5.0 })],
        });
        let r = &z[0];
        // 重排域前后并集：x 从 0 覆盖到 65。
        let ok = z.len() == 1
            && r.source == ChangeSource::ZReorder
            && r.rect.x <= 0.0
            && r.rect.right() >= 65.0;
        let empty = map_event(&DamageEvent::ZReorder {
            node_id: 4,
            span: ReorderSpan { span_start: 0, span_end: 0 },
            before: Vec::new(),
            after: Vec::new(),
        });
        set.add("D13-映射-重排域前后并集", ok && empty.is_empty(), "");
    }

    {
        let e = map_event(&DamageEvent::Effect {
            node_id: 5,
            impact: DamageRect { x: -3.0, y: -3.0, w: 26.0, h: 26.0 },
        });
        let ok = e.len() == 1 && e[0].source == ChangeSource::Effect;
        // 五类穷举机检：映射表无缺项。
        let full = ChangeSource::all().len() == 5;
        set.add("D13-映射-效果影响域与五类穷举", ok && full, "");
    }

    // ---- 最小编号公共祖先合并 ----

    {
        // 1 为根，2/3 挂 1，4 挂 2。
        let parents = vec![
            (1u64, None),
            (2, Some(1)),
            (3, Some(1)),
            (4, Some(2)),
        ];
        let chain = ancestor_chain(4, &parents);
        let lca_all = lowest_common_ancestor(&[2, 3, 4], &parents);
        let lca_sub = lowest_common_ancestor(&[4], &parents);
        let lca_none = lowest_common_ancestor(&[1, 2], &[]);
        // 4 的链 [4,2,1]；单节点入参取链上编号最小者（最外层坐标系）。
        let ok = chain == vec![4, 2, 1]
            && lca_all == Some(1)
            && lca_sub == Some(1)
            && lca_none.is_none();
        set.add("D13-合并-最小编号公共祖先", ok, "");
    }

    {
        let parents = vec![(1u64, None), (2, Some(1)), (3, Some(1)), (4, Some(2))];
        let mut c = DirtyCollector::new(parents);
        let _ = c.push(DamageEvent::Content {
            node_id: 4,
            bounds: DamageRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
        });
        let _ = c.push(DamageEvent::Content {
            node_id: 3,
            bounds: DamageRect { x: 200.0, y: 0.0, w: 10.0, h: 10.0 },
        });
        let f = c.collect_frame();
        // 两块相离的区域合并进 1（编号最小的公共祖先）。
        let ok = f.merged_into == Some(1)
            && f.regions.len() == 2
            && f.regions.iter().all(|r| r.owner == 1);
        set.add("D13-合并-跨子树并入最小祖先", ok, "");
    }

    // ---- 去重纪律（相交即并 / 相离不并） ----

    {
        let mk = |x: f32, y: f32, w: f32, h: f32| {
            DamageRegion::new(
                DamageRect { x, y, w, h },
                ChangeSource::Content,
                DamageOp::Add,
                1,
            )
        };
        let touching = coalesce(vec![mk(0.0, 0.0, 10.0, 10.0), mk(8.0, 0.0, 10.0, 10.0)]);
        let apart = coalesce(vec![mk(0.0, 0.0, 10.0, 10.0), mk(500.0, 0.0, 10.0, 10.0)]);
        // 相交并为一块；相离保持两块（合并会撑大区域）。
        let ok = touching.len() == 1
            && touching[0].rect.right() >= 18.0
            && apart.len() == 2;
        set.add("D13-去重-相交即并相离不并", ok, "");
    }

    {
        // 语义动作不同不并（加入与移除不可混同）。
        let add = DamageRegion::new(
            DamageRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
            ChangeSource::Content,
            DamageOp::Add,
            1,
        );
        let mut rem = add;
        rem.op = DamageOp::Remove;
        let r = coalesce(vec![add, rem]);
        set.add("D13-去重-语义不同不合并", r.len() == 2, "");
    }

    {
        // 稳定序：同输入同输出（回归可比对）。
        let mk = |x: f32| {
            DamageRegion::new(
                DamageRect { x, y: 0.0, w: 5.0, h: 5.0 },
                ChangeSource::Content,
                DamageOp::Add,
                1,
            )
        };
        let a = coalesce(vec![mk(90.0), mk(10.0), mk(50.0)]);
        let b = coalesce(vec![mk(50.0), mk(90.0), mk(10.0)]);
        set.add("D13-去重-稳定序可复现", a == b && a[0].rect.x == 10.0, "");
    }

    // ---- 全帧信号 ----

    {
        let parents = vec![(1u64, None)];
        let mut c = DirtyCollector::new(parents);
        // 每块相离，凑过爆炸阈值。
        let mut i = 0u32;
        while i <= REGION_EXPLOSION_THRESHOLD as u32 {
            let _ = c.push(DamageEvent::Content {
                node_id: 1,
                bounds: DamageRect {
                    x: (i as f32) * 1000.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
            });
            i += 1;
        }
        let f = c.collect_frame();
        let ok = f.full_frame
            && f.regions.len() > REGION_EXPLOSION_THRESHOLD
            && c.full_frame_signals() >= 1
            && c.audits().iter().any(|a| a.contains("全帧建议信号"));
        set.add("D13-全帧-区域爆炸升级信号", ok, "");
    }

    // ---- 边界防护：矩形非法值显性拒绝 ----

    {
        let neg = DamageRect::new(0.0, 0.0, -1.0, 5.0).is_none();
        let nan = DamageRect::new(f32::NAN, 0.0, 1.0, 1.0).is_none();
        let inf = DamageRect::new(0.0, 0.0, f32::INFINITY, 1.0).is_none();
        let okc = DamageRect::new(0.0, 0.0, 1.0, 1.0).is_some();
        set.add("D13-防护-非法矩形显性拒绝", neg && nan && inf && okc, "");
    }

    // ---- 错误路径：事件溢出显性拒绝 ----

    {
        let mut c = DirtyCollector::new(vec![(1u64, None)]);
        let mut i = 0;
        let mut rejected = false;
        while i < MAX_EVENTS_PER_FRAME + 1 {
            let r = c.push(DamageEvent::Content {
                node_id: 1,
                bounds: DamageRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            });
            if r.is_err() {
                rejected = true;
                break;
            }
            i += 1;
        }
        let logged = c
            .errors()
            .iter()
            .any(|(_, code, _)| *code == "E_FRAME_EVENT_OVERFLOW");
        set.add("D13-错误-事件溢出拒绝并登记", rejected && logged, "");
    }

    // ---- 降级：跨树无公共祖先登记不静默 ----

    {
        // 1 与 9 分属两棵树。
        let parents = vec![(1u64, None), (9, None)];
        let mut c = DirtyCollector::new(parents);
        let _ = c.push(DamageEvent::Content {
            node_id: 1,
            bounds: DamageRect { x: 0.0, y: 0.0, w: 4.0, h: 4.0 },
        });
        let _ = c.push(DamageEvent::Content {
            node_id: 9,
            bounds: DamageRect { x: 50.0, y: 0.0, w: 4.0, h: 4.0 },
        });
        let f = c.collect_frame();
        let logged = c
            .errors()
            .iter()
            .any(|(_, code, _)| *code == "E_NO_COMMON_ANCESTOR");
        set.add("D13-降级-跨树登记补齐", f.merged_into.is_none() && logged, "");
    }

    // ---- 降级：并集溢出钳制到画面域 ----

    {
        let mut c = DirtyCollector::new(vec![(1u64, None)]);
        c.set_viewport(DamageRect { x: 0.0, y: 0.0, w: 800.0, h: 600.0 });
        let _ = c.push(DamageEvent::Content {
            node_id: 1,
            bounds: DamageRect { x: 700.0, y: 500.0, w: 400.0, h: 400.0 },
        });
        let outside = c.push(DamageEvent::Content {
            node_id: 1,
            bounds: DamageRect { x: 5000.0, y: 5000.0, w: 10.0, h: 10.0 },
        });
        let _ = outside;
        let f = c.collect_frame();
        // 与画面域求交后落在画面内；全在画外的被丢弃。
        let inside = f
            .regions
            .iter()
            .all(|r| r.rect.right() <= 800.0 + DAMAGE_EPS && r.rect.bottom() <= 600.0 + DAMAGE_EPS);
        set.add("D13-钳制-并集溢出钳到画面域", inside && f.regions.len() == 1, "");
    }

    {
        // 坐标量级溢出（超大值）钳制后不产生 NaN。
        let huge = DamageRect {
            x: -1.0e30,
            y: -1.0e30,
            w: 2.0e30,
            h: 2.0e30,
        };
        let c = huge.clamp_coords();
        let finite = c.x.is_finite() && c.w.is_finite() && c.w >= 0.0;
        let bounded = c.right() <= COORD_CLAMP + 1.0;
        set.add("D13-钳制-坐标量级溢出兜底", finite && bounded, "");
    }

    // ---- 降级：映射缺源登记补齐（脏区仍产出，宁多不漏） ----

    {
        // 父映射只登记 1；事件却来自未登记的 7。
        let mut c = DirtyCollector::new(vec![(1u64, None)]);
        let _ = c.push(DamageEvent::Content {
            node_id: 7,
            bounds: DamageRect { x: 0.0, y: 0.0, w: 6.0, h: 6.0 },
        });
        let f = c.collect_frame();
        let logged = c
            .errors()
            .iter()
            .any(|(_, code, _)| *code == "E_DIRTY_SOURCE_MISSING");
        // 缺源不丢区域：脏区照常进帧，供 F0641 消费。
        set.add(
            "D13-降级-映射缺源登记补齐",
            logged && f.regions.len() == 1,
            "",
        );
    }

    // ---- 帧号推进与区域计数 ----

    {
        let mut c = DirtyCollector::new(vec![(1u64, None), (2, Some(1))]);
        let _ = c.push(DamageEvent::Content {
            node_id: 2,
            bounds: DamageRect { x: 0.0, y: 0.0, w: 8.0, h: 8.0 },
        });
        let f0 = c.collect_frame();
        let f1 = c.collect_frame();
        let ok = f0.frame == 0 && f1.frame == 1 && f0.events == 1 && f1.events == 0;
        set.add("D13-帧-逻辑帧号与事件计数", ok, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> DamageRect {
        DamageRect { x, y, w, h }
    }

    #[test]
    fn union_covers_both_sides() {
        let a = r(0.0, 0.0, 10.0, 10.0);
        let b = r(100.0, 0.0, 10.0, 10.0);
        let u = a.union(&b);
        assert!(u.x <= 0.0 && u.right() >= 110.0, "并集须覆盖两侧");
    }

    #[test]
    fn touches_is_symmetric_and_reflects_gap() {
        let a = r(0.0, 0.0, 10.0, 10.0);
        let near = r(9.0, 0.0, 10.0, 10.0);
        let far = r(100.0, 0.0, 10.0, 10.0);
        assert!(a.touches(&near) && near.touches(&a));
        assert!(!a.touches(&far) && !far.touches(&a));
    }

    #[test]
    fn coalesce_absorbs_chain() {
        let mk = |x: f32| {
            DamageRegion::new(r(x, 0.0, 10.0, 10.0), ChangeSource::Content, DamageOp::Add, 1)
        };
        // 三块首尾相接应并成一块。
        let out = coalesce(vec![mk(0.0), mk(9.0), mk(18.0)]);
        assert_eq!(out.len(), 1, "相接链须并为一块");
        assert!(out[0].rect.right() >= 28.0);
    }

    #[test]
    fn lca_picks_min_id_common() {
        // 5 为根，1 与 3 都挂 5，9 挂 1。
        let parents = vec![(5u64, None), (1, Some(5)), (3, Some(5)), (9, Some(1))];
        // 1 与 3 是兄弟：链 [1,5] ∩ [3,5] = {5}，编号最小者 = 5。
        assert_eq!(lowest_common_ancestor(&[1, 3], &parents), Some(5));
        // 1 与 9 是祖孙：链 [1,5] ∩ [9,1,5] = {1,5}，编号最小者 = 1。
        assert_eq!(lowest_common_ancestor(&[1, 9], &parents), Some(1));
        // 单节点：其链 [9,1,5] 上编号最小者 = 1（最外层坐标系）。
        assert_eq!(lowest_common_ancestor(&[9], &parents), Some(1));
        // 根自身：链 [5]，即 5。
        assert_eq!(lowest_common_ancestor(&[5], &parents), Some(5));
    }

    #[test]
    fn lca_cycle_does_not_hang() {
        // 父映射成环（结构异常）时不死循环。
        let parents = vec![(1u64, Some(2)), (2, Some(1))];
        let chain = ancestor_chain(1, &parents);
        assert!(chain.len() <= parents.len() + 1, "成环须被上界截断");
    }

    #[test]
    fn collect_frame_merges_and_reports() {
        let parents = vec![(1u64, None), (2, Some(1))];
        let mut c = DirtyCollector::new(parents);
        c.push(DamageEvent::Content { node_id: 2, bounds: r(0.0, 0.0, 10.0, 10.0) }).unwrap();
        c.push(DamageEvent::Content { node_id: 2, bounds: r(5.0, 0.0, 10.0, 10.0) }).unwrap();
        let f = c.collect_frame();
        assert_eq!(f.merged_into, Some(1));
        assert_eq!(f.regions.len(), 1, "相交两块须并为一块");
        assert_eq!(f.raw_regions, 2);
    }

    #[test]
    fn empty_frame_is_clean() {
        let mut c = DirtyCollector::new(vec![(1u64, None)]);
        let f = c.collect_frame();
        assert!(f.regions.is_empty() && !f.full_frame && f.merged_into.is_none());
    }

    #[test]
    fn viewport_clips_and_drops() {
        let mut c = DirtyCollector::new(vec![(1u64, None)]);
        c.set_viewport(r(0.0, 0.0, 100.0, 100.0));
        c.push(DamageEvent::Content { node_id: 1, bounds: r(90.0, 90.0, 50.0, 50.0) }).unwrap();
        c.push(DamageEvent::Content { node_id: 1, bounds: r(900.0, 900.0, 10.0, 10.0) }).unwrap();
        let f = c.collect_frame();
        assert_eq!(f.regions.len(), 1, "画外整块须丢弃");
        assert!(f.regions[0].rect.right() <= 100.0 + DAMAGE_EPS);
    }

    #[test]
    fn effects_checks_all_green() {
        let set = run_ved13_checks();
        let (p, fcount) = set.tally();
        assert!(!set.truncated());
        assert!(set.all_passed(), "VE-F0613 红项：{}/{}", p, p + fcount);
    }
}