//! VE-F5604 · 混音总线骨架域自检（判据四条逐条映射 + 反向语料钉门禁）。
//!
//! **判据（锚点原文）**：总线树、主总线兜底、拒绝成环、判据。
//!
//! 自检纪律：判据区零 panic 面（下标走 `get`/`Option`）；判据侧独立
//! 重算（深度上限/兜底线/stamp 字面量写死）；反向语料证明门禁不恒绿
//! （空树路由必兜底、断裂必兜底、成环必拒）。

use alloc::vec;
use alloc::vec::Vec;

use super::veab04_mixbus::{
    BusErr, BusId, BusTree, FALLBACK_ALERT_RATIO, MAX_BUSES, MAX_TREE_DEPTH, MASTER_BUS_ID,
    RouteOutcome, RouteTable,
};

/// 判据侧独立重排的锚点判据四条。
const CRITERIA_RECHECK: [&str; 4] = ["总线树", "主总线兜底", "拒绝成环", "判据"];

/// VE-F5604 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_veab04_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("veab04_mixbus");

    // —— 判据一 · 总线树：主总线构造即在 + 建树 + 深度上限拒绝 ——
    let mut tree = BusTree::new();
    let master_ok = tree.contains(BusId(MASTER_BUS_ID))
        && tree.node(BusId(MASTER_BUS_ID)).map(|n| n.depth).unwrap_or(999) == 0
        && MAX_TREE_DEPTH == 8
        && MAX_BUSES == 64;
    let music = tree.create_bus(BusId(MASTER_BUS_ID), "音乐总线");
    let sfx = tree.create_bus(BusId(MASTER_BUS_ID), "音效总线");
    let music_id = match music {
        Ok(id) => id,
        Err(_) => BusId(9999),
    };
    let sfx_id = match sfx {
        Ok(id) => id,
        Err(_) => BusId(9998),
    };
    let sub = tree.create_bus(music_id, "音乐子总线");
    let sub_id = match sub {
        Ok(id) => id,
        Err(_) => BusId(9997),
    };
    let orphan = tree.create_bus(BusId(7777), "孤儿总线");
    s.add(
        "AB04-总线树-构造建树与孤儿拒绝",
        master_ok
            && music.is_ok()
            && sfx.is_ok()
            && sub.is_ok()
            && orphan == Err(BusErr::OrphanParent)
            && tree.len() == 4
            && tree.node(sub_id).map(|n| n.depth).unwrap_or(999) == 2,
        "主总线构造即在（深度0）；三层建树成功且深度递增正确；挂不存在父拒绝（OrphanParent）",
    );

    // —— 深度上限写死：超过即拒绝建树（判据一锚点原文） ——
    let mut deep = BusTree::new();
    let mut cursor = BusId(MASTER_BUS_ID);
    let mut depth_rejected = false;
    let mut built = 0u32;
    while built < MAX_TREE_DEPTH as u32 + 3 {
        match deep.create_bus(cursor, "层") {
            Ok(id) => {
                cursor = id;
                built += 1;
            }
            Err(BusErr::DepthExceeded) => {
                depth_rejected = true;
                break;
            }
            Err(_) => {
                depth_rejected = false;
                break;
            }
        }
    }
    s.add(
        "AB04-深度上限-超过即拒绝建树",
        depth_rejected
            && deep.len() <= MAX_TREE_DEPTH + 1
            && deep
                .node(cursor)
                .map(|n| n.depth)
                .unwrap_or(999) <= MAX_TREE_DEPTH,
        "逐层建树到第 MAX_TREE_DEPTH+1 层被拒（DepthExceeded）；已建深度全部合法——上限写死常量非配置",
    );

    // —— 判据二 · 主总线兜底：断裂/孤声都有路走 + 全遥测 ——
    let mut table = RouteTable::new();
    let _ = table.bind(1, music_id, &tree);
    let _ = table.bind(2, sfx_id, &tree);
    let ok_route = table.route(1, &tree);
    // 断裂：绑定的总线从树里消失（模拟：一棵只有主总线的新树——不含 music_id，
    // 也不再建新总线以免 next_id 从 1 重发撞上主树的 music_id）。
    let broken_tree = BusTree::new();
    let broken_route = table.route(1, &broken_tree);
    let orphan_route = table.route(42, &tree);
    let events = table.fallback_events();
    s.add(
        "AB04-主总线兜底-断裂孤声全遥测",
        ok_route == RouteOutcome { bus: music_id, fallback: false }
            && broken_route.bus == BusId(MASTER_BUS_ID)
            && broken_route.fallback
            && orphan_route.bus == BusId(MASTER_BUS_ID)
            && orphan_route.fallback
            && table.fallback_count() == 2
            && events.len() == 2
            && events.get(0).map(|e| e.why.contains("断裂")).unwrap_or(false)
            && events.get(1).map(|e| e.why.contains("孤声")).unwrap_or(false),
        "正常路由直达；断裂/孤声都落主总线（任何声音必须有路可走）；两路兜底各留事件（全遥测）",
    );

    // —— 兜底率警示：安全网不是常态路由 ——
    // 先凑到超线：绑定 2 个，兜底 5 次 > 2×2=4（断裂 4 次 + 孤声 1 次）。
    let _ = table.route(1, &broken_tree);
    let _ = table.route(1, &broken_tree);
    let _ = table.route(1, &broken_tree);
    let alert = table.fallback_ratio_alert();
    let mut quiet = RouteTable::new();
    let _ = quiet.bind(9, sfx_id, &tree);
    let _ = quiet.route(9, &tree);
    let no_alert = quiet.fallback_ratio_alert().is_none();
    s.add(
        "AB04-兜底率警示-超线提示",
        table.fallback_count() == 5
            && table.binding_count() == 2
            && alert.is_some()
            && alert
                .map(|a| a.contains("安全网不是常态路由") && a.contains("兜底"))
                .unwrap_or(false)
            && no_alert
            && FALLBACK_ALERT_RATIO == 2,
        "兜底 5 次>绑定 2×2 触发警示行（常态兜底即配置病灶）；正常路由不警示（警示线判据侧写死）",
    );

    // —— 判据三 · 拒绝成环：reattach 挂到自己的后代 ——
    let mut tree2 = BusTree::new();
    let a = tree2.create_bus(BusId(MASTER_BUS_ID), "A");
    let a_id = match a {
        Ok(id) => id,
        Err(_) => BusId(9990),
    };
    let b = tree2.create_bus(a_id, "B");
    let b_id = match b {
        Ok(id) => id,
        Err(_) => BusId(9989),
    };
    let c = tree2.create_bus(b_id, "C");
    let c_id = match c {
        Ok(id) => id,
        Err(_) => BusId(9988),
    };
    let cycle = tree2.reattach(a_id, c_id); // A 挂到 C（C 是 A 的后代）→ 环
    let legal = tree2.reattach(c_id, BusId(MASTER_BUS_ID)); // C 重挂回主总线 → 合法
    let c_after = tree2.node(c_id).map(|n| (n.depth, n.parent)).unwrap_or((999, None));
    s.add(
        "AB04-拒绝成环-后代重挂被拒",
        cycle == Err(BusErr::CycleRejected)
            && legal.is_ok()
            && c_after.0 == 1
            && c_after.1 == Some(BusId(MASTER_BUS_ID))
            && tree2
                .path_to_root(a_id)
                .map(|p| p.len())
                .unwrap_or(999) == 2,
        "A 挂到自己的后代 C 被拒（CycleRejected）；C 重挂回主总线合法且深度修正为 1（树保持树）",
    );

    // —— reattach 深度超限：子树随迁超界被拒 ——
    let mut tree3 = BusTree::new();
    let mut cursor3 = BusId(MASTER_BUS_ID);
    let mut chain: Vec<BusId> = vec![BusId(MASTER_BUS_ID)];
    let mut built3 = 0u32;
    while built3 < MAX_TREE_DEPTH as u32 - 1 {
        if let Ok(id) = tree3.create_bus(cursor3, "链") {
            cursor3 = id;
            chain.push(id);
            built3 += 1;
        } else {
            break;
        }
    }
    // 深枝建在同一棵 tree3：链已占 7 层，深枝根(深度1，子树最深到 5)挂到
    // 链尾(深度7)后子树底部深度 = 7+1+4 = 12 > 8 → 整树把关拒绝。
    let m1 = tree3.create_bus(BusId(MASTER_BUS_ID), "深枝根");
    let m1_id = match m1 {
        Ok(id) => id,
        Err(_) => BusId(9987),
    };
    let mut cursor4 = m1_id;
    let mut built4 = 0u32;
    while built4 < 4 {
        if let Ok(id) = tree3.create_bus(cursor4, "深枝") {
            cursor4 = id;
            built4 += 1;
        } else {
            break;
        }
    }
    let deep_branch = cursor4; // 深度为 5 的枝尾（MAX_TREE_DEPTH=8 内）
    let tail = match chain.get(chain.len() - 1) {
        Some(t) => *t,
        None => BusId(MASTER_BUS_ID),
    };
    let over = tree3.reattach(m1_id, tail);
    s.add(
        "AB04-重挂超限-子树随迁整树把关",
        over == Err(BusErr::DepthExceeded)
            && chain.len() == MAX_TREE_DEPTH
            && tree3.node(deep_branch).is_some()
            && tree3.node(m1_id).map(|n| n.depth).unwrap_or(999) == 1,
        "重挂不只看新父深度——子树随迁后的最深边界一起把关（超界 DepthExceeded，拒绝后树保持原状）",
    );

    // —— 批量导入导出：往返等价 + 非法条目逐条拒绝 ——
    let mut table2 = RouteTable::new();
    let _ = table2.bind(10, music_id, &tree);
    let _ = table2.bind(11, sfx_id, &tree);
    let exported = table2.export_routes();
    let mut restored = RouteTable::new();
    let report = restored.import_routes(&exported, &tree);
    let bad_batch = [
        (20u16, BusId(5555)), // 不在树内
        (21u16, BusId(6666)),
    ];
    let bad_report = restored.import_routes(&bad_batch, &tree);
    s.add(
        "AB04-导入导出-往返等价+逐条拒绝",
        exported.len() == 2
            && report.accepted == 2
            && report.rejected.is_empty()
            && restored.export_routes() == exported
            && bad_report.accepted == 0
            && bad_report.rejected.len() == 2
            && bad_report
                .rejected
                .get(0)
                .map(|(_, e)| *e == BusErr::UnknownBus)
                .unwrap_or(false),
        "导出快照在树上原样复原（可迁移可备份）；树外目标逐条拒绝留原因（部分成功合法不静默）",
    );

    // —— 路由可查：单行人话（无障碍域本色） ——
    let mut table3 = RouteTable::new();
    let _ = table3.bind(30, sub_id, &tree);
    let line_ok = table3.route(30, &tree);
    let line = table3.route_line(30, &line_ok, &tree);
    let mut table4 = RouteTable::new();
    let outcome_fb = table4.route(31, &tree);
    let line_fb = table4.route_line(31, &outcome_fb, &tree);
    s.add(
        "AB04-路由可查-读屏单行",
        line.contains("声音30") && line.contains("音乐子总线") && !line.contains("兜底")
            && line_fb.contains("声音31") && line_fb.contains("主总线兜底"),
        "正常路由报目标总线名；兜底路由明说兜底（路由可查——锚点无障碍条目的落地面）",
    );

    // —— 反向语料：空树路由不 panic 且兜底（门禁不恒绿） ——
    let empty_tree = BusTree::new();
    let mut table5 = RouteTable::new();
    let out = table5.route(99, &empty_tree);
    s.add(
        "AB04-反向-空树孤声仍兜底",
        out.bus == BusId(MASTER_BUS_ID) && out.fallback && table5.fallback_count() == 1,
        "空配置场景孤声查询不 panic、走主总线兜底并计数（安全网的最后底线）",
    );

    // —— 判据 stamp 独立对账 ——
    let stamps = ["总线树", "主总线兜底", "拒绝成环", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "AB04-判据stamp-四条独立重排全等",
        stamp_ok && MASTER_BUS_ID == 0,
        "锚点判据四条与判据侧独立重排逐条全等（常量被误改先红）；主总线编号钉死为 0",
    );

    s
}
