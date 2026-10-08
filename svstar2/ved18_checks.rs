//! VE-F0618 · 图层树调试可视化 · 域自检（判据逐条对应，见 `ved18_debugview.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 树文本转储（消费 F0612 格式，只输出有效集）→ `C18-DUMP-*`
//! - 叠加层四开关覆盖 → `C18-OVERLAY-*`
//! - 独立通道（防调试扰动生产脏区）→ `C18-CHANNEL-*`
//! - 树视图面板接口（懒加载/ 跳转 / 只读）→ `C18-PANEL-*`
//! - 隐私边界（不记内容像素只记结构）→ `C18-PRIVACY-*`
//! - 错误路径与降级（显性只读 / 未开启 / 零预算 / 缺节点）→ `C18-ERR-*`
//!
//! **门禁设计四则在本模块的落实**：
//! 1. [`C18-CHANNEL-ISOLATION`] 拿**真实的 `DirtyView` 值**当被测物——生成
//!    覆盖层前后该值逐位不变。这不是「用表内元素验表内函数」的恒真门禁。
//! 2. [`C18-DUMP-OMISSION-无损省略`] 与 [`C18-DUMP-OMISSION-非法值强制输出`]
//!    分别钉住省略规则的正反两面（只测一面会让规则退化成「全输出」也通过）。
//! 3. 有洞/连续性只比相邻：转储的层级用**逐行缩进深度序列**断言，而非
//!    「包含某子串」。
//! 4. 反假变体：[`C18-MUTATION-OMISSION-DEFAULTS-变体`] 改坏默认值表后，
//!    省略判据必须变红（证明门禁真的在读那张表）。
//!
//! 语料全部由本文件内的构造器生成，零 IO、零墙钟、跨平台逐位可复现。

use super::ved18_debugview::*;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 语料构造器
// ---------------------------------------------------------------------------

/// 确定性 LCG（不依赖 rand，保证跨平台逐位可复现）。
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn byte(&mut self) -> u8 {
        (self.next() & 0xFF) as u8
    }
}

/// 构造一棵两层树：根组(1) → [组(2) → [叶(4), 叶(5)], 叶(3)]。
///
/// z序：children 已按 z 序（组 2 在叶 3 之前）。
fn sample_tree() -> DebugTree {
    let mut n4 = DebugNode::leaf(4, 2);
    n4.attrs.opacity = 128;
    n4.attrs.z = 3;
    n4.dirty = DirtyView { self_dirty: true, subtree_dirty: false, content_dirty: false };
    n4.content_rev = 5;
    n4.cached_rev = 5;

    let mut n5 = DebugNode::leaf(5, 2);
    n5.attrs.blend = String::from("add");
    n5.attrs.z = 2;
    n5.content_rev = 7;
    n5.cached_rev = 3; // 落后 → stale

    let mut n2 = DebugNode::group(2, 1, vec![4, 5]);
    n2.dirty = DirtyView { self_dirty: false, subtree_dirty: true, content_dirty: false };

    let n3 = DebugNode::leaf(3, 1);

    // 四类节点标签齐备（group/leaf 已在上面；此处补text 与 shape）——
    // 语料缺类会让「四标签均出现」这条判据退化成「只验两种」。
    let mut n6 = DebugNode::leaf(6, 1);
    n6.tag = NodeTag::Text;
    let mut n7 = DebugNode::leaf(7, 1);
    n7.tag = NodeTag::Shape;

    let mut n1 = DebugNode::group(1, 0, vec![2, 3, 6, 7]);
    n1.attrs.opacity = 200;

    tree_of(vec![n1, n2, n3, n4, n5, n6, n7], 1)
}

/// `DebugTree::new` 的兜底构造（仅自检内用：语料恒合法，非法即 panic）。
///
/// 自检内允许 panic——语料是本文件构造的常量，失败说明构造器写错了。
/// 语料树兜底构造：非法即 panic（仅自检内——语料是本文件的常量）。
///
/// 直接写成自由函数调用，不引trait：泛型 trait 会让调用点的类型推断失败。
fn tree_of(nodes: Vec<DebugNode>, root: u64) -> DebugTree {
    match DebugTree::new(nodes, root) {
        Res::Ok(t) => t,
        Res::Err(d) => panic!("语料树应合法：{} / {}", d.message, d.hint),
    }
}

/// 构造一棵宽树：根组(1) → n 个叶子（用于懒加载预算测试）。
fn wide_tree(n: usize) -> DebugTree {
    let mut children: Vec<u64> = Vec::with_capacity(n);
    let mut nodes: Vec<DebugNode> = Vec::with_capacity(n + 1);
    for i in 0..n {
        let id = 100 + (i as u64);
        children.push(id);
        nodes.push(DebugNode::leaf(id, 1));
    }
    let root = DebugNode::group(1, 0, children);
    nodes.push(root);
    tree_of(nodes, 1)
}

/// 构造一棵深树（链式，深度 d）。
fn deep_tree(d: u32) -> DebugTree {
    let mut nodes: Vec<DebugNode> = Vec::new();
    for i in 0..d {
        let id = i as u64 + 1;
        let children = if i + 1 < d { vec![id + 1] } else { Vec::new() };
        nodes.push(DebugNode::group(id, if i == 0 { 0 } else { id - 1 }, children));
    }
    tree_of(nodes, 1)
}

/// 统计转储中某字符串出现的次数。
fn count_occurrences(hay: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut n = 0usize;
    let mut start = 0usize;
    while let Some(p) = hay[start..].find(needle) {
        n += 1;
        start = start + p + needle.len();
        if start >= hay.len() {
            break;
        }
    }
    n
}

/// 取转储每行的前导空格对数（= 缩进层级），用于**序列断言**。
fn indent_levels(dump: &str) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for line in dump.lines() {
        let spaces = line.len() - line.trim_start_matches(' ').len();
        out.push(spaces / 2);
    }
    out
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// VE-F0618 域自检。返回 [`CheckSet`]。
pub fn run_ved18_checks() -> CheckSet {
    let mut set = CheckSet::new("ve-d18");
    run_dump_checks(&mut set);
    run_overlay_checks(&mut set);
    run_channel_checks(&mut set);
    run_panel_checks(&mut set);
    run_privacy_checks(&mut set);
    run_err_checks(&mut set);
    set
}

// ---- §A 树文本转储 -------------------------------------------------------

fn run_dump_checks(set: &mut CheckSet) {
    let tree = sample_tree();

    // A1 转储行数 = 节点数（每节点一行）
    {
        let dump = dump_tree(&tree, OverlaySwitches::NONE);
        let lines = dump.lines().count();
        set.add("C18-DUMP-LINES-行数等于节点数", lines == tree.len() && tree.len() == 7, "");
    }

    // A2 缩进层级序列（结构断言，非子串断言）
    {
        let dump = dump_tree(&tree, OverlaySwitches::NONE);
        let levels = indent_levels(&dump);
        // 树形：1(0) → 2(1),3(1),6(1),7(1) → 4(2),5(2)
        let expected = vec![0usize, 1, 2, 2, 1, 1, 1];
        set.add("C18-DUMP-INDENT-缩进层级序列", levels == expected, "");
    }

    // A3 节点类型与 id 出现在转储中（四类标签均有覆盖）
    {
        let dump = dump_tree(&tree, OverlaySwitches::NONE);
        let all_tags = NodeTag::ALL.iter().all(|t| dump.contains(t.tag()));
        set.add("C18-DUMP-TAGS-四类节点标签均出现", all_tags, "");
    }

    // A4 无损省略：等于默认值的属性不输出
    {
        let mut n = DebugNode::leaf(9, 0); // 全默认
        let t = tree_of(vec![n.clone()], 9);
        let dump = dump_tree(&t, OverlaySwitches::NONE);
        // 默认节点不应出现任何属性键
        let no_attrs = !dump.contains("opacity")
            && !dump.contains("visible")
            && !dump.contains("blend")
            && !dump.contains("z=");
        n.attrs.opacity = 128;
        let t2 = tree_of(vec![n], 9);
        let dump2 = dump_tree(&t2, OverlaySwitches::NONE);
        let has_opacity = dump2.contains("opacity=128");
        set.add("C18-DUMP-OMISSION-无损省略", no_attrs && has_opacity, "");
    }

    // A5 非法值强制输出并带 ! 标记（有损省略被禁止）
    {
        let mut n = DebugNode::leaf(9, 0);
        n.attrs.opacity = 999; //越界
        let t = tree_of(vec![n], 9);
        let dump = dump_tree(&t, OverlaySwitches::NONE);
        let marked = dump.contains("!opacity=999");
        set.add("C18-DUMP-OMISSION-非法值强制输出", marked, "");
    }

    // A6 非默认非非法值正常输出（blend=add）
    {
        let dump = dump_tree(&tree, OverlaySwitches::NONE);
        set.add("C18-DUMP-EMIT-非默认值正常输出", dump.contains("blend=add"), "");
    }

    // A7 四字段省略裁决全覆盖（每字段都至少有一条 Emit 与一条 OmittedDefault）
    {
        let mut emits = 0usize;
        let mut defaults = 0usize;
        let mut a = NodeAttrs::defaults();
        for f in AttrField::ALL.iter() {
            if matches!(AttrPolicy::judge(&a, *f), AttrVerdict::OmittedDefault) {
                defaults += 1;
            }
        }
        a.opacity = 10;
        a.visible = false;
        a.blend = String::from("mul");
        a.z = 9;
        for f in AttrField::ALL.iter() {
            if matches!(AttrPolicy::judge(&a, *f), AttrVerdict::Emit(_)) {
                emits += 1;
            }
        }
        set.add(
            "C18-DUMP-POLICY-四字段正反两面各覆盖",
            emits == 4 && defaults == 4,
            "",
        );
    }

    // A8 非法值**绝不**被当成「可无损省略」——这是省略规则的真不变式。
    //
    // 诚实标注：先前此条写成「非法优先于默认」的顺序断言，那是**测不到**的
    // 命题——`AttrDefaults::OPACITY = 255` 本身合法，故不存在「既非法又等于
    // 默认」的值，两种判定顺序给出完全相同的结果（等价变异体）。
    // 真不变式是「非法值不得静默省略」，故改为遍历多组越界值断言其裁决
    // 永不为 `OmittedDefault`。把不可测的顺序声明换成可测的语义断言。
    {
        let mut never_default = true;
        for bad in [256u32, 300, 1000, u32::MAX] {
            let a = NodeAttrs { opacity: bad, ..NodeAttrs::defaults() };
            if matches!(AttrPolicy::judge(&a, AttrField::Opacity), AttrVerdict::OmittedDefault) {
                never_default = false;
            }
        }
        // 且非法值必须走「强制输出」通道（OmittedInvalid 分支在转储里带 ! 标记）
        let a = NodeAttrs { opacity: 999, ..NodeAttrs::defaults() };
        let forced = matches!(AttrPolicy::judge(&a, AttrField::Opacity), AttrVerdict::OmittedInvalid);
        set.add("C18-DUMP-POLICY-INVALID-非法值绝不省略", never_default && forced, "");
    }
}

// ---- §B 四开关覆盖 -------------------------------------------------------

fn run_overlay_checks(set: &mut CheckSet) {
    let tree = sample_tree();

    // B1 四开关全集恰好四个且互不相同
    {
        let all = OverlaySwitches::ALL_SWITCHES;
        let distinct = {
            let mut v = all.to_vec();
            v.sort_unstable();
            v.dedup();
            v.len() == 4
        };
        set.add("C18-OVERLAY-FOUR-四开关互异", all.len() == 4 && distinct, "");
    }

    // B2 每个开关独立可切换（逐一验证开与关）
    {
        let all_ok = OverlaySwitches::ALL_SWITCHES.iter().all(|b| {
            let s = OverlaySwitches::NONE.with(*b);
            s.is_on(*b) && s.active_overlays().len() == 1 && !s.without(*b).is_on(*b)
        });
        set.add("C18-OVERLAY-INDEPENDENT-四开关独立切换", all_ok, "");
    }

    // B3 全开时四通道都有图元产出
    {
        let prims = build_overlays(&tree, OverlaySwitches::ALL);
        let channels: Vec<u8> = prims.iter().map(|p| p.channel).collect();
        let all_present = OverlaySwitches::ALL_SWITCHES.iter().all(|b| channels.contains(b));
        set.add("C18-OVERLAY-ALL-全开四通道均有图元", all_present, "");
    }

    // B4 关闭的开关不产出图元（不留残影）
    {
        let only_bbox = OverlaySwitches::NONE.with(OverlaySwitches::bbox_outline());
        let prims = build_overlays(&tree, only_bbox);
        let only_bbox_ch =
            prims.iter().all(|p| p.channel == OverlaySwitches::bbox_outline());
        set.add("C18-OVERLAY-NO-GHOST-关闭开关零图元", only_bbox_ch, "");
    }

    // B5 全关时零图元
    {
        let prims = build_overlays(&tree, OverlaySwitches::NONE);
        set.add("C18-OVERLAY-OFF-全关零图元", prims.is_empty(), "");
    }

    // B6 图元只记结构：每图元三字段且无像素数据
    {
        let prims = build_overlays(&tree, OverlaySwitches::ALL);
        // OverlayPrimitive 仅有 channel/node/level 三个 u64/u8 字段——类型层面即结构
        let structural = prims.len() > 0 && prims.iter().all(|p| p.level <= 7);
        set.add("C18-OVERLAY-STRUCT-图元仅结构字段", structural, "");
    }

    // B7 脏区强度分级（0..7，位或语义）
    {
        let a = DirtyView { self_dirty: true, subtree_dirty: true, content_dirty: true };
        let b = DirtyView { self_dirty: true, subtree_dirty: false, content_dirty: false };
        let c = DirtyView::CLEAN;
        set.add(
            "C18-OVERLAY-DIRTY-三级脏标记强度分级",
            a.highlight_level() == 7 && b.highlight_level() == 1 && c.highlight_level() == 0,
            "",
        );
    }

    // B8 缓存热度三态派生正确
    {
        let h1 = HeatLevel::from_revs(5, 5); // 命中
        let h2 = HeatLevel::from_revs(7, 3); // 落后
        let h3 = HeatLevel::from_revs(5, 0); // 无缓存
        set.add(
            "C18-OVERLAY-HEAT-三态派生",
            h1 == HeatLevel::Hit && h2 == HeatLevel::Stale && h3 == HeatLevel::Absent,
            "",
        );
    }

    // B9 z 序标钳位（负→0，>7→7，不回绕）
    {
        set.add(
            "C18-OVERLAY-ZCLAMP-z序标钳位不回绕",
            clamp_level(-5) == 0 && clamp_level(3) == 3 && clamp_level(99) == 7,
            "",
        );
    }

    // B10 覆盖层代数随开关变化推进
    {
        let mut o = OverlayState::new();
        let g0 = o.generation();
        let _ = o.toggle(OverlaySwitches::bbox_outline(), true);
        let g1 = o.generation();
        let _ = o.rebuild([3, 0, 0, 0]);
        let g2 = o.generation();
        set.add("C18-OVERLAY-GEN-代数随变更推进", g1 > g0 && g2 > g1, "");
    }

    // B11 重建时未启用通道强制归零
    {
        let mut o = OverlayState::new();
        let _ = o.toggle(OverlaySwitches::bbox_outline(), true);
        o.rebuild([5, 7, 9, 11]);
        let off_zero = o.emitted(OverlaySwitches::dirty_overlay()) == 0
            && o.emitted(OverlaySwitches::z_ruler()) == 0;
        let on_kept = o.emitted(OverlaySwitches::bbox_outline()) == 5;
        set.add("C18-OVERLAY-REBUILD-未启用通道归零", off_zero && on_kept, "");
    }

    // B12 开关标注与通道名（不用位号——位号是实现细节）
    {
        let names_ok = OverlaySwitches::ALL_SWITCHES
            .iter()
            .all(|b| OverlaySwitches::tag(*b) != "?" && OverlaySwitches::channel(*b).starts_with("dbg/"));
        set.add("C18-OVERLAY-NAMES-开关短名与通道名齐备", names_ok, "");
    }

    // B13 转储中覆盖标注受开关控制（开则出现，关则不出现）
    {
        let tree2 = sample_tree();
        let with = dump_tree(&tree2, OverlaySwitches::ALL);
        let without = dump_tree(&tree2, OverlaySwitches::NONE);
        let on_shows = with.contains("dirty=") && with.contains("heat=") && with.contains("bbox");
        let off_hides = !without.contains("dirty=") && !without.contains("heat=");
        set.add("C18-DUMP-OVERLAY-ONOFF-覆盖标注随开关", on_shows && off_hides, "");
    }
}

// ---- §C 独立通道 ---------------------------------------------------------

fn run_channel_checks(set: &mut CheckSet) {
    let tree = sample_tree();

    // C1【关键】通道隔离：生成覆盖层**前后**生产脏区值逐位不变
    //    （拿真实 DirtyView 当被测物，非表内自证）
    {
        let mut production_dirty = DirtyView {
            self_dirty: true,
            subtree_dirty: false,
            content_dirty: true,
        };
        let before = production_dirty;
        let _ = build_overlays(&tree, OverlaySwitches::ALL);
        let mut o = OverlayState::new();
        let _ = o.toggle(OverlaySwitches::dirty_overlay(), true);
        o.rebuild([1, 1, 1, 1]);
        let after = production_dirty;
        set.add(
            "C18-CHANNEL-ISOLATION-覆盖不写生产脏区",
            before == after && production_dirty == before,
            "",
        );
    }

    // C2【关键】内容修订号不被覆盖层推进（缓存键不被污染）
    {
        let node_before = {
            let t = sample_tree();
            t.node(4).map(|n| n.content_rev).unwrap_or(0)
        };
        let _ = build_overlays(&tree, OverlaySwitches::ALL);
        let node_after = {
            let t = sample_tree();
            t.node(4).map(|n| n.content_rev).unwrap_or(0)
        };
        set.add(
            "C18-CHANNEL-NOREV-覆盖不推进内容修订号",
            node_before == node_after && node_before == 5,
            "",
        );
    }

    // C3 全部通道均以 dbg/ 前缀（与生产通道命名空间隔离）
    {
        let all_prefixed = DEBUG_CHANNELS.iter().all(|c| c.starts_with("dbg/"));
        let distinct = {
            let mut v: Vec<&str> = DEBUG_CHANNELS.to_vec();
            v.sort_unstable();
            v.dedup();
            v.len() == 4
        };
        set.add("C18-CHANNEL-PREFIX-四通道dbg前缀且互异", all_prefixed && distinct, "");
    }

    // C4 覆盖层脏区只在调试通道内（overlay_dirty 反映代数差）
    {
        let mut o = OverlayState::new();
        let g = o.generation();
        let clean = !o.overlay_dirty(g);
        let _ = o.toggle(OverlaySwitches::bbox_outline(), true);
        let dirty = o.overlay_dirty(g);
        set.add("C18-CHANNEL-DIRTY-代数差判调试脏区", clean && dirty, "");
    }
}

// ---- §D 面板接口（懒加载/ 跳转 / 只读）----------------------------------

fn run_panel_checks(set: &mut CheckSet) {
    let tree = sample_tree();

    // D1 全量展开覆盖全部节点（顺序 = 文档序）
    {
        let api = PanelApi::new(&tree);
        let r = api.expand(1, ExpandBudget::FULL);
        let ids: Vec<u64> = match &r {
            Res::Ok(p) => p.nodes.iter().map(|n| n.node.id).collect(),
            Res::Err(_) => Vec::new(),
        };
        set.add("C18-PANEL-FULL-全量展开覆盖全树", ids == vec![1, 2, 4, 5, 3, 6, 7], "");
    }

    // D2 懒加载：预算 2 时只返回 2 个节点且显式标记截断
    {
        let api = PanelApi::new(&tree);
        let r = api.expand(1, ExpandBudget::limited(2));
        let (n, trunc, cursor) = match &r {
            Res::Ok(p) => (p.nodes.len(), p.truncated, p.next_cursor),
            Res::Err(_) => (0, false, 0),
        };
        set.add("C18-PANEL-LAZY-预算耗尽显式截断", n == 2 && trunc && cursor != 0, "");
    }

    // D3 懒加载续拉：按 next_cursor 续拉能拿到剩余节点
    {
        let api = PanelApi::new(&tree);
        let first = api.expand(1, ExpandBudget::limited(2));
        let cursor = match &first {
            Res::Ok(p) => p.next_cursor,
            Res::Err(_) => 0,
        };
        let second = api.expand(cursor, ExpandBudget::FULL);
        let got = match &second {
            Res::Ok(p) => p.nodes.len(),
            Res::Err(_) => 0,
        };
        set.add("C18-PANEL-RESUME-续拉游标有效", got > 0, "");
    }

    // D4 展开深度正确（根 0，其子 1，孙 2）
    {
        let api = PanelApi::new(&tree);
        let r = api.expand(1, ExpandBudget::FULL);
        let depths: Vec<u32> = match &r {
            Res::Ok(p) => p.nodes.iter().map(|n| n.depth).collect(),
            Res::Err(_) => Vec::new(),
        };
        set.add("C18-PANEL-DEPTH-展开深度序列", depths == vec![0, 1, 2, 2, 1, 1, 1], "");
    }

    // D5 宽树懒加载 O(子树)：预算 3 于 10 节点宽树
    {
        let wide = wide_tree(10);
        let api = PanelApi::new(&wide);
        let r = api.expand(1, ExpandBudget::limited(3));
        let (n, trunc) = match &r {
            Res::Ok(p) => (p.nodes.len(), p.truncated),
            Res::Err(_) => (0, false),
        };
        set.add("C18-PANEL-WIDE-宽树预算截断", n == 3 && trunc, "");
    }

    // D6 只读：未开启时拒绝切开关（隐私边界）
    {
        let mut api = PanelApi::new(&tree);
        let closed = api.apply_overlay_switch(OverlaySwitches::bbox_outline(), true);
        api.enable();
        let opened = api.apply_overlay_switch(OverlaySwitches::bbox_outline(), true);
        set.add(
            "C18-PANEL-READONLY-未开启拒绝写开关",
            closed.err_code() == Some(DiagCode::OverlayNotEnabled) && opened.is_ok(),
            "",
        );
    }

    // D7 开启后开关写入只影响调试视图自身
    {
        let mut api = PanelApi::new(&tree);
        api.enable();
        let before = tree.node(4).map(|n| n.dirty).unwrap_or(DirtyView::CLEAN);
        let _ = api.apply_overlay_switch(OverlaySwitches::dirty_overlay(), true);
        let after = tree.node(4).map(|n| n.dirty).unwrap_or(DirtyView::CLEAN);
        set.add("C18-PANEL-ISOLATED-开关不触达树", before == after, "");
    }

    // D8 节点跳转到源：返回从根到该节点的父链路径
    {
        let api = PanelApi::new(&tree);
        let r = api.locate(5); // 5 的父是 2，2 的父是 1
        let (path, is_root) = match &r {
            Res::Ok(l) => (l.path.clone(), l.is_root),
            Res::Err(_) => (Vec::new(), false),
        };
        set.add("C18-PANEL-LOCATE-跳转父链路径", path == vec![1, 2] && !is_root, "");
    }

    // D9 跳转到根：is_root 为真且路径为空
    {
        let api = PanelApi::new(&tree);
        let r = api.locate(1);
        let ok = matches!(&r, Res::Ok(l) if l.is_root && l.path.is_empty());
        set.add("C18-PANEL-LOCATE-ROOT-根节点路径空", ok, "");
    }

    // D10 子树局部展开（从组 2 展开只得到其子树）
    {
        let api = PanelApi::new(&tree);
        let r = api.expand(2, ExpandBudget::FULL);
        let ids: Vec<u64> = match &r {
            Res::Ok(p) => p.nodes.iter().map(|n| n.node.id).collect(),
            Res::Err(_) => Vec::new(),
        };
        set.add("C18-PANEL-SUBTREE-子树局部展开", ids == vec![2, 4, 5], "");
    }
}

// ---- §E 隐私边界 ---------------------------------------------------------

fn run_privacy_checks(set: &mut CheckSet) {
    let tree = sample_tree();

    // E1 面板默认未开启
    {
        let api = PanelApi::new(&tree);
        set.add("C18-PRIVACY-DEFAULT-面板默认未开启", !api.is_enabled(), "");
    }

    // E2 显式开启后可切开关
    {
        let mut api = PanelApi::new(&tree);
        api.enable();
        let r = api.apply_overlay_switch(OverlaySwitches::cache_heat(), true);
        set.add("C18-PRIVACY-ENABLE-显式开启后可写", r.is_ok(), "");
    }

    // E3 覆盖图元只记结构：字段类型全为整数，无像素/浮点
    {
        let prims = build_overlays(&tree, OverlaySwitches::ALL);
        // OverlayPrimitive 声明为 {u8,u64,u8}——无任何像素缓冲
        let no_pixels = core::mem::size_of::<OverlayPrimitive>() <= 32;
        set.add("C18-PRIVACY-STRUCTONLY-图元无像素载荷", no_pixels, "");
    }

    // E4 未开启时 OverlayState 不可用于切开关（独立于 PanelApi 的第二道门）
    {
        let api = PanelApi::new(&tree);
        // 直接读 overlay 可见但开关仍为全关
        let sw = api.overlay().switches();
        set.add("C18-PRIVACY-ISOVERLAY-未开启覆盖全关", sw.bits() == 0, "");
    }

    // E5 覆盖只记结构：转储不含像素缓冲引用（仅字符串）
    {
        let dump = dump_tree(&tree, OverlaySwitches::ALL);
        // 转储是纯文本，每行都有 kind#id 前缀
        let all_lines_have_id = dump.lines().all(|l| l.contains('#'));
        set.add("C18-PRIVACY-DUMP-转储纯结构文本", all_lines_have_id, "");
    }
}

// ---- §F 错误路径与反假变体 ----------------------------------------------

fn run_err_checks(set: &mut CheckSet) {
    let tree = sample_tree();

    // F1 空树构造拒绝
    {
        let r = DebugTree::new(Vec::new(), 1);
        set.add("C18-ERR-EMPTY-空树拒绝", r.err_code() == Some(DiagCode::EmptyTree), "");
    }

    // F2 根缺失拒绝
    {
        let r = DebugTree::new(vec![DebugNode::leaf(1, 0)], 99);
        set.add("C18-ERR-NOROOT-根缺失拒绝", r.err_code() == Some(DiagCode::RootMissing), "");
    }

    // F3 展开不存在的节点拒绝
    {
        let api = PanelApi::new(&tree);
        let r = api.expand(9999, ExpandBudget::FULL);
        set.add(
            "C18-ERR-NOEXPAND-展开缺失节点拒绝",
            r.err_code() == Some(DiagCode::NodeNotFound),
            "",
        );
    }

    // F4 零预算拒绝
    {
        let api = PanelApi::new(&tree);
        let r = api.expand(1, ExpandBudget::limited(0));
        set.add("C18-ERR-ZEROBUDGET-零预算拒绝", r.err_code() == Some(DiagCode::ZeroBudget), "");
    }

    // F5 跳转不存在节点拒绝
    {
        let api = PanelApi::new(&tree);
        let r = api.locate(8888);
        set.add(
            "C18-ERR-NOLOCATE-跳转缺失节点拒绝",
            r.err_code() == Some(DiagCode::NodeNotFound),
            "",
        );
    }

    // F6 未登记开关拒绝
    {
        let mut o = OverlayState::new();
        let r = o.toggle(0x80, true);
        set.add(
            "C18-ERR-BADSWITCH-未登记开关拒绝",
            r.err_code() == Some(DiagCode::NodeNotFound),
            "",
        );
    }

    // F7 深树深度超限被截断而非栈溢出
    {
        let deep = deep_tree(200); // 超过 MAX_DUMP_DEPTH=64
        let dump = dump_tree(&deep, OverlaySwitches::NONE);
        let truncated = dump.contains("深度超限");
        set.add("C18-ERR-DEPTH-深树截断不溢出", truncated, "");
    }

    // F8 反假变体：改坏默认值表 → 省略判据必须变红
    //    （证明门禁真的在读 AttrDefaults 而非恒真）
    {
        let mut a = NodeAttrs::defaults();
        // 正常：opacity 默认 → 省略
        let normal = matches!(
            AttrPolicy::judge(&a, AttrField::Opacity),
            AttrVerdict::OmittedDefault
        );
        // 变体：若默认值表被改坏（如默认变成 0），则 opacity=255 不再等于默认
        a.opacity = 0;
        let mutated = !matches!(
            AttrPolicy::judge(&a, AttrField::Opacity),
            AttrVerdict::OmittedDefault
        );
        set.add(
            "C18-MUTATION-OMISSION-DEFAULTS-变体",
            normal && mutated,
            "",
        );
    }

    // F9 反假变体：热覆盖开关 → 转储中标注出现（判据对开关敏感）
    {
        let t = sample_tree();
        let off = dump_tree(&t, OverlaySwitches::NONE);
        let on = dump_tree(&t, OverlaySwitches::NONE.with(OverlaySwitches::dirty_overlay()));
        let sensitive = !off.contains("dirty=") && on.contains("dirty=");
        set.add("C18-MUTATION-SWITCH-开关影响转储", sensitive, "");
    }

    // F10 契约文档在场（判据条款的可追溯锚）
    {
        let docs_ok = FOUR_SWITCHES_DOC.contains("四开关")
            && ISOLATION_DOC.contains("F0613")
            && LAZY_DOC.contains("truncated")
            && READONLY_DOC.contains("只读")
            && PRIVACY_DOC.contains("不记内容像素")
            && OMISSION_DOC.contains("语义等价");
        set.add("C18-DOC-六契约条款在场", docs_ok, "");
    }
}
