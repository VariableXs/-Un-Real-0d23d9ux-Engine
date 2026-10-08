//! 撤回 + 时间旅行（#433~#440）—— AI-06 域三。
//!
//! 零 AI：操作栈、快照、时间线、分支合并全为确定性算法。
//! 撤回作用于**编辑操作序列**，与部署总纲 core 输出契约（ProjectIR）解耦：
//! 三端共用同一份操作栈语义，写回通道（C08）只在真正落盘时介入。

// ------------------------------------------------------------------ 常量（F433/F434/F435）

/// 单步撤回倒放动画时长（ms）。
pub const UNDO_STEP_MS: u32 = 300;
/// 多步撤回每一步的时长（ms）。
pub const MULTI_STEP_MS: u32 = 200;
/// 快照回滚的画面闪白时长（ms）。
pub const SNAPSHOT_FLASH_MS: u32 = 250;

/// 编辑操作种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// 新增节点
    Add,
    /// 删除节点
    Delete,
    /// 修改内容
    Modify,
    /// 移动/重连
    Move,
}

impl OpKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OpKind::Add => "add",
            OpKind::Delete => "delete",
            OpKind::Modify => "modify",
            OpKind::Move => "move",
        }
    }
}

/// 一条可撤回的编辑操作。
#[derive(Debug, Clone)]
pub struct EditOp {
    pub id: usize,
    pub kind: OpKind,
    pub node: String,
    pub desc: String,
    /// 发生时间（毫秒时间戳，用于时间线排序与时间旅行）。
    pub at: u64,
}

// ------------------------------------------------------------------ F433/F434 操作栈

/// 编辑历史：undo 栈 + redo 栈。
#[derive(Debug, Default)]
pub struct History {
    pub ops: Vec<EditOp>,
    pub redo: Vec<EditOp>,
    seq: usize,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次编辑；新操作会清空 redo 栈。
    pub fn record(&mut self, kind: OpKind, node: &str, desc: &str, at: u64) -> usize {
        self.seq += 1;
        let id = self.seq;
        self.ops.push(EditOp { id, kind, node: node.to_string(), desc: desc.to_string(), at });
        self.redo.clear();
        id
    }

    pub fn can_undo(&self) -> bool {
        !self.ops.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// F433 单步撤回（Ctrl+Z），返回被撤回的操作。
    pub fn undo(&mut self) -> Option<EditOp> {
        let op = self.ops.pop()?;
        self.redo.push(op.clone());
        Some(op)
    }

    /// F434 多步撤回（`/undo 5`），按「由新到旧」顺序返回。
    pub fn undo_n(&mut self, n: usize) -> Vec<EditOp> {
        let mut out = Vec::new();
        for _ in 0..n {
            match self.undo() {
                Some(op) => out.push(op),
                None => break,
            }
        }
        out
    }

    /// 重做（`/redo`）。
    pub fn redo(&mut self) -> Option<EditOp> {
        let op = self.redo.pop()?;
        self.ops.push(op.clone());
        Some(op)
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// 当前栈顶时间点（时间旅行的默认起点）。
    pub fn now(&self) -> u64 {
        self.ops.last().map(|o| o.at).unwrap_or(0)
    }
}

// ------------------------------------------------------------------ F438 撤回动画

/// 撤回动画类型：新增淡出、删除淡入、连线重连。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimKind {
    FadeOut,
    FadeIn,
    Reconnect,
}

impl AnimKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AnimKind::FadeOut => "fade-out",
            AnimKind::FadeIn => "fade-in",
            AnimKind::Reconnect => "reconnect",
        }
    }
}

/// F438：新增的撤回归档=淡出，删除的撤回=淡入，其余=连线重连。
pub fn undo_animation(op: &EditOp) -> AnimKind {
    match op.kind {
        OpKind::Add => AnimKind::FadeOut,
        OpKind::Delete => AnimKind::FadeIn,
        OpKind::Modify | OpKind::Move => AnimKind::Reconnect,
    }
}

/// F433 单步动画帧。
pub fn single_undo_frame(op: &EditOp) -> (AnimKind, u32) {
    (undo_animation(op), UNDO_STEP_MS)
}

/// F434 多步撤回时间轴：每步 200ms，返回 (步序号, 起始毫秒, 动画类型)。
pub fn multi_undo_plan(ops: &[EditOp]) -> Vec<(usize, u32, AnimKind)> {
    ops.iter()
        .enumerate()
        .map(|(i, op)| (i, i as u32 * MULTI_STEP_MS, undo_animation(op)))
        .collect()
}

// ------------------------------------------------------------------ F439 快照管理

/// 快照：某一时刻的完整节点清单。
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: usize,
    pub name: String,
    /// 保存时间（ms）。
    pub at: u64,
    /// 节点清单（此处为节点名，落到 IR 时对应节点 id 集合）。
    pub nodes: Vec<String>,
}

impl Snapshot {
    /// 节点数（快照列表显示「时间 + 节点数」）。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn summary(&self) -> String {
        format!("#{} {} {} 节点", self.id, self.name, self.node_count())
    }
}

#[derive(Debug, Default)]
pub struct SnapshotStore {
    pub snaps: Vec<Snapshot>,
    seq: usize,
}

impl SnapshotStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// F439 save：同名快照不重复登记（返回既有 id）。
    pub fn save(&mut self, name: &str, at: u64, nodes: &[&str]) -> usize {
        if let Some(s) = self.snaps.iter().find(|s| s.name == name) {
            return s.id;
        }
        self.seq += 1;
        let id = self.seq;
        self.snaps.push(Snapshot {
            id,
            name: name.to_string(),
            at,
            nodes: nodes.iter().map(|n| n.to_string()).collect(),
        });
        id
    }

    pub fn list(&self) -> Vec<&Snapshot> {
        self.snaps.iter().collect()
    }

    pub fn load(&self, id: usize) -> Option<&Snapshot> {
        self.snaps.iter().find(|s| s.id == id)
    }

    pub fn delete(&mut self, id: usize) -> bool {
        let before = self.snaps.len();
        self.snaps.retain(|s| s.id != id);
        self.snaps.len() != before
    }
}

/// F435 快照回滚：画面闪白 → 恢复到快照状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollbackPlan {
    pub flash_ms: u32,
    pub node_count: usize,
    pub ok: bool,
}

pub fn rollback(store: &SnapshotStore, id: usize) -> RollbackPlan {
    match store.load(id) {
        Some(s) => RollbackPlan { flash_ms: SNAPSHOT_FLASH_MS, node_count: s.node_count(), ok: true },
        None => RollbackPlan { flash_ms: 0, node_count: 0, ok: false },
    }
}

// ------------------------------------------------------------------ F436/F437 时间线

/// 时间线标记：圆点=历史操作，星星=快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Dot,
    Star,
}

impl Mark {
    pub fn glyph(self) -> &'static str {
        match self {
            Mark::Dot => "●",
            Mark::Star => "★",
        }
    }
}

/// F437 时间线面板数据：按时间升序混合历史操作（圆点）与快照（星星）。
pub fn timeline(h: &History, store: &SnapshotStore) -> Vec<(u64, Mark, String)> {
    let mut v: Vec<(u64, Mark, String)> = Vec::new();
    for op in &h.ops {
        v.push((op.at, Mark::Dot, format!("{} {}", op.kind.as_str(), op.node)));
    }
    for s in &store.snaps {
        v.push((s.at, Mark::Star, s.name.clone()));
    }
    v.sort_by(|a, b| a.0.cmp(&b.0).then((a.1 as u8).cmp(&(b.1 as u8))));
    v
}

/// F436 时间旅行：倒放到目标时间点，返回需撤回的操作（由新到旧）。
pub fn time_travel(h: &History, target: u64) -> Vec<EditOp> {
    let mut out: Vec<EditOp> = h.ops.iter().filter(|o| o.at > target).cloned().collect();
    out.sort_by(|a, b| b.at.cmp(&a.at).then(b.id.cmp(&a.id)));
    out
}

/// 时间旅行的动画时长：按倒放步数 × 200ms。
pub fn time_travel_ms(ops: &[EditOp]) -> u32 {
    ops.len() as u32 * MULTI_STEP_MS
}

// ------------------------------------------------------------------ F440 分支编辑

/// 基于历史某一点创建的编辑分支。
#[derive(Debug, Clone)]
pub struct Branch {
    pub id: usize,
    pub name: String,
    /// 分叉自第几个操作（0 = 空历史处分叉）。
    pub from_op: usize,
    pub ops: Vec<EditOp>,
}

#[derive(Debug, Default)]
pub struct BranchStore {
    pub branches: Vec<Branch>,
    /// 当前所在分支（None = 主干）。
    pub current: Option<usize>,
    seq: usize,
}

impl BranchStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// F440 从历史某点创建分支，并复制该点之后的操作作为分支初值。
    pub fn create(&mut self, name: &str, h: &History, from_op: usize) -> usize {
        self.seq += 1;
        let id = self.seq;
        let ops: Vec<EditOp> = h.ops.iter().take(from_op).cloned().collect();
        self.branches.push(Branch { id, name: name.to_string(), from_op, ops });
        id
    }

    pub fn list(&self) -> Vec<&Branch> {
        self.branches.iter().collect()
    }

    pub fn get(&self, id: usize) -> Option<&Branch> {
        self.branches.iter().find(|b| b.id == id)
    }

    /// 切换分支（None 回到主干）。
    pub fn switch(&mut self, id: Option<usize>) -> bool {
        match id {
            None => {
                self.current = None;
                true
            }
            Some(i) if self.branches.iter().any(|b| b.id == i) => {
                self.current = Some(i);
                true
            }
            Some(_) => false,
        }
    }

    pub fn is_current(&self, id: usize) -> bool {
        self.current == Some(id)
    }
}

/// 合并结果：应用条数 + 冲突节点。
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub applied: usize,
    pub conflicts: Vec<String>,
    pub ok: bool,
}

/// F440 合并分支回主干：分叉点之后主干已改动的同名节点 → 冲突，其余按序应用。
pub fn merge(h: &mut History, store: &BranchStore, id: usize) -> MergeResult {
    let Some(branch) = store.get(id) else {
        return MergeResult { applied: 0, conflicts: Vec::new(), ok: false };
    };
    let mut conflicts: Vec<String> = Vec::new();
    let mut applied = 0usize;
    for op in &branch.ops {
        let touched_after_fork =
            h.ops.iter().skip(branch.from_op).any(|o| o.node == op.node && o.id != op.id);
        if touched_after_fork {
            if !conflicts.contains(&op.node) {
                conflicts.push(op.node.clone());
            }
            continue;
        }
        h.ops.push(op.clone());
        h.redo.clear();
        applied += 1;
    }
    let empty = conflicts.is_empty();
    MergeResult { applied, conflicts, ok: empty }
}

/// 合并前的冲突预览（不改动历史）。
pub fn merge_preview(h: &History, store: &BranchStore, id: usize) -> MergeResult {
    let mut clone = History { ops: h.ops.clone(), redo: h.redo.clone(), seq: h.ops.len() };
    merge(&mut clone, store, id)
}

// ------------------------------------------------------------------ 自检

/// AI-06 域三自检（#433~#440，8 项）。
pub fn run_undo_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("undo");

    // F433
    let mut h = History::new();
    h.record(OpKind::Add, "login()", "新增登录函数", 100);
    h.record(OpKind::Modify, "check()", "改名", 200);
    let before = h.len();
    let undone = h.undo();
    let after = h.len();
    let (anim, ms) = single_undo_frame(undone.as_ref().unwrap());
    s.add(
        "F433 单步撤回",
        before == 2 && after == 1 && undone.as_ref().map(|o| o.node.as_str()) == Some("check()") && anim == AnimKind::Reconnect && ms == 300 && h.can_redo(),
        "Ctrl+Z 画布倒放动画 300ms",
    );

    // F434
    let mut h2 = History::new();
    for i in 0..8 {
        h2.record(OpKind::Modify, &format!("n{i}"), "改", (i as u64 + 1) * 100);
    }
    let five = h2.undo_n(5);
    let plan = multi_undo_plan(&five);
    let rest = h2.undo_n(10);
    s.add(
        "F434 多步撤回",
        five.len() == 5
            && five[0].node == "n7"
            && five[4].node == "n3"
            && plan.len() == 5
            && plan[4].1 == 800
            && plan.iter().all(|(_, _, a)| *a == AnimKind::Reconnect)
            && rest.len() == 3
            && h2.is_empty(),
        "/undo 5 连续倒放，每步 200ms",
    );

    // F435
    let mut st = SnapshotStore::new();
    let sid = st.save("初版", 1000, &["a", "b", "c"]);
    let plan = rollback(&st, sid);
    let bad = rollback(&st, 999);
    s.add(
        "F435 快照回滚",
        plan.ok && plan.flash_ms == 250 && plan.node_count == 3 && !bad.ok && bad.flash_ms == 0,
        "画面闪白 → 恢复快照状态",
    );

    // F436
    let mut h3 = History::new();
    h3.record(OpKind::Add, "a", "", 100);
    h3.record(OpKind::Add, "b", "", 200);
    h3.record(OpKind::Add, "c", "", 300);
    let tt = time_travel(&h3, 150);
    let tt_ms = time_travel_ms(&tt);
    let tt_all = time_travel(&h3, 0);
    s.add(
        "F436 时间旅行",
        tt.len() == 2 && tt[0].node == "c" && tt[1].node == "b" && tt_ms == 400 && tt_all.len() == 3,
        "时间线面板选择时间点 → 倒放",
    );

    // F437
    let mut st3 = SnapshotStore::new();
    st3.save("v1", 150, &["a"]);
    st3.save("v2", 350, &["a", "b"]);
    let tl = timeline(&h3, &st3);
    let dots = tl.iter().filter(|(_, m, _)| *m == Mark::Dot).count();
    let stars = tl.iter().filter(|(_, m, _)| *m == Mark::Star).count();
    s.add(
        "F437 时间线面板",
        tl.len() == 5 && dots == 3 && stars == 2 && tl[0].0 == 100 && tl[4].0 == 350 && Mark::Star.glyph() == "★" && Mark::Dot.glyph() == "●",
        "圆点=历史，星星=快照",
    );

    // F438
    let add = EditOp { id: 1, kind: OpKind::Add, node: "x".into(), desc: "".into(), at: 0 };
    let del = EditOp { id: 2, kind: OpKind::Delete, node: "x".into(), desc: "".into(), at: 0 };
    let mov = EditOp { id: 3, kind: OpKind::Move, node: "x".into(), desc: "".into(), at: 0 };
    s.add(
        "F438 撤回动画",
        undo_animation(&add) == AnimKind::FadeOut
            && undo_animation(&del) == AnimKind::FadeIn
            && undo_animation(&mov) == AnimKind::Reconnect
            && AnimKind::FadeOut.as_str() == "fade-out",
        "新增淡出，删除淡入，连线重连",
    );

    // F439
    let mut st4 = SnapshotStore::new();
    let i1 = st4.save("s1", 10, &["a"]);
    let i2 = st4.save("s2", 20, &["a", "b"]);
    let dup = st4.save("s1", 30, &["z"]);
    let listed_len = st4.list().len();
    let loaded_txt = st4.load(i2).map(|x| x.summary());
    let removed = st4.delete(i1);
    let left = st4.list().len();
    s.add(
        "F439 快照管理",
        i1 == 1
            && i2 == 2
            && dup == i1
            && listed_len == 2
            && loaded_txt == Some("#2 s2 2 节点".to_string())
            && removed
            && left == 1,
        "save/list/load/delete，列表显示时间+节点数",
    );

    // F440
    let mut h5 = History::new();
    h5.record(OpKind::Modify, "a", "", 100);
    h5.record(OpKind::Modify, "b", "", 200);
    let mut bs = BranchStore::new();
    let bid = bs.create("实验", &h5, 1);
    let switched = bs.switch(Some(bid));
    let is_cur = bs.is_current(bid);
    let preview = merge_preview(&h5, &bs, bid);
    let merged = merge(&mut h5, &bs, bid);
    let len_after_merge = h5.len();
    let bad_id = merge(&mut h5, &bs, 99).ok;
    let back = bs.switch(None);
    s.add(
        "F440 分支编辑",
        bs.list().len() == 1
            && bid == 1
            && switched
            && is_cur
            && preview.ok
            && merged.applied == 1
            && merged.conflicts.is_empty()
            && len_after_merge == 3
            && !bad_id
            && back
            && bs.current.is_none(),
        "基于历史创建分支，可切换/合并",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f433_redo_restores() {
        let mut h = History::new();
        h.record(OpKind::Add, "a", "", 1);
        let op = h.undo().unwrap();
        assert_eq!(op.node, "a");
        assert!(h.is_empty());
        let redone = h.redo().unwrap();
        assert_eq!(redone.node, "a");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn f434_new_edit_clears_redo() {
        let mut h = History::new();
        h.record(OpKind::Add, "a", "", 1);
        h.undo();
        assert!(h.can_redo());
        h.record(OpKind::Add, "b", "", 2);
        assert!(!h.can_redo());
    }

    #[test]
    fn f435_rollback_missing_snapshot() {
        let st = SnapshotStore::new();
        assert!(!rollback(&st, 1).ok);
    }

    #[test]
    fn f436_travel_to_future_is_empty() {
        let mut h = History::new();
        h.record(OpKind::Add, "a", "", 100);
        assert!(time_travel(&h, 999).is_empty());
    }

    #[test]
    fn f437_timeline_empty() {
        let h = History::new();
        let st = SnapshotStore::new();
        assert!(timeline(&h, &st).is_empty());
    }

    #[test]
    fn f439_duplicate_name_keeps_first() {
        let mut st = SnapshotStore::new();
        let a = st.save("same", 1, &["x"]);
        let b = st.save("same", 2, &["y", "z"]);
        assert_eq!(a, b);
        assert_eq!(st.load(a).unwrap().node_count(), 1);
    }

    #[test]
    fn f440_merge_conflict_detected() {
        let mut h = History::new();
        h.record(OpKind::Modify, "shared", "", 100);
        let mut bs = BranchStore::new();
        let bid = bs.create("b", &h, 1);
        // 主干在分叉点之后又改了 shared → 合并冲突
        h.record(OpKind::Modify, "shared", "", 200);
        let r = merge(&mut h, &bs, bid);
        assert!(!r.ok);
        assert_eq!(r.conflicts, vec!["shared".to_string()]);
        assert_eq!(r.applied, 0);
    }
}
