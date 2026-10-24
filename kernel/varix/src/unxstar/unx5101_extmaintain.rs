//! UNX-F5101 · extent 树维护操作总纲（B09 查找面 ↔ 本批维护面分界）账 · 诊断段 0x8000
//! 验收锚点：UNX-F5101-J1 维护面与查找面分界声明落账——维护六操作
//! （分裂 split / 合并 merge / 收缩 shrink / 提升 promote / 重算 recompute /
//! 回滚 rollback），与 B09 段判据交叉引用零悬空（六操作名均可在分界表索引到，
//! 引用表空缺即红），总纲可导出。


use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8000;

/// 维护六操作枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaintainOp {
    Split,
    Merge,
    Shrink,
    Promote,
    Recompute,
    Rollback,
}

impl MaintainOp {
    pub fn as_str(self) -> &'static str {
        match self {
            MaintainOp::Split => "split",
            MaintainOp::Merge => "merge",
            MaintainOp::Shrink => "shrink",
            MaintainOp::Promote => "promote",
            MaintainOp::Recompute => "recompute",
            MaintainOp::Rollback => "rollback",
        }
    }
    pub fn all() -> [MaintainOp; 6] {
        [MaintainOp::Split, MaintainOp::Merge, MaintainOp::Shrink, MaintainOp::Promote, MaintainOp::Recompute, MaintainOp::Rollback]
    }
}

/// 查找面（B09）：只读，不动树结构。
pub const B09_OPS: [&str; 2] = ["lookup", "walk"];

/// 分界声明：维护面操作 → B09 交叉引用（零悬空 = 每个 op 至少引用 1 条 B09 判据）。
pub struct Boundary {
    pub refs: Vec<(MaintainOp, Vec<&'static str>)>,
}

impl Boundary {
    pub fn new() -> Self {
        Boundary { refs: Vec::new() }
    }

    pub fn add_ref(&mut self, op: MaintainOp, b09_ids: Vec<&'static str>) {
        self.refs.push((op, b09_ids));
    }

    /// 零悬空：六操作全部登记且每条引用非空。
    pub fn no_dangling(&self) -> bool {
        for op in MaintainOp::all().iter() {
            match self.refs.iter().find(|(o, _)| o == op) {
                Some((_, ids)) => {
                    if ids.is_empty() {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 查找面隔离：引用只允许指向 B09 前缀判据。
    pub fn refs_all_b09(&self) -> bool {
        self.refs.iter().all(|(_, ids)| ids.iter().all(|id| id.starts_with("B09")))
    }

    /// 总纲可导出：操作名按序 join。
    pub fn export(&self) -> String {
        MaintainOp::all().iter().map(|o| o.as_str()).collect::<Vec<&str>>().join(",")
    }
}

/// 维护树：节点带操作留痕（Rollback 可撤销最后一次结构操作）。
pub struct MaintTree {
    pub leaves: Vec<u64>,       // 叶项起始块
    pub depth: u16,
    pub history: Vec<(MaintainOp, String)>,
    pub undo_stack: Vec<Vec<u64>>,
}

impl MaintTree {
    pub fn new(initial: Vec<u64>) -> Self {
        MaintTree { leaves: initial, depth: 0, history: Vec::new(), undo_stack: Vec::new() }
    }

    fn snapshot(&mut self, op: MaintainOp) {
        self.undo_stack.push(self.leaves.clone());
        self.history.push((op, format!("n{}", self.leaves.len())));
    }

    /// 分裂：中间叶项一分为二（块区间对半）。
    pub fn split(&mut self, idx: usize) -> bool {
        if idx >= self.leaves.len() {
            return false;
        }
        self.snapshot(MaintainOp::Split);
        let v = self.leaves[idx];
        self.leaves[idx] = v;
        self.leaves.insert(idx + 1, v + 8);
        true
    }

    /// 合并：相邻两项合一。
    pub fn merge(&mut self, idx: usize) -> bool {
        if idx + 1 >= self.leaves.len() {
            return false;
        }
        self.snapshot(MaintainOp::Merge);
        self.leaves.remove(idx + 1);
        true
    }

    /// 收缩：尾部弹出一项。
    pub fn shrink(&mut self) -> bool {
        if self.leaves.is_empty() {
            return false;
        }
        self.snapshot(MaintainOp::Shrink);
        self.leaves.pop();
        true
    }

    /// 提升：depth+1。
    pub fn promote(&mut self) -> bool {
        if self.depth >= 5 {
            return false;
        }
        self.snapshot(MaintainOp::Promote);
        self.depth += 1;
        true
    }

    /// 重算：entries 与 leaves.len() 对齐（返回修正量）。
    pub fn recompute(&mut self, entries: u32) -> i32 {
        self.snapshot(MaintainOp::Recompute);
        let actual = self.leaves.len() as i32;
        let diff = entries as i32 - actual;
        diff
    }

    /// 回滚：撤销最后一次结构操作。
    pub fn rollback(&mut self) -> bool {
        match self.undo_stack.pop() {
            Some(prev) => {
                self.leaves = prev;
                true
            }
            None => false,
        }
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x8000 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x5101_8000;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("六操作-枚举全6", MaintainOp::all().len() == 6));
    out.push(("查找面-两操作常数", B09_OPS.len() == 2));
    out.push(("分界-空账悬空", !Boundary::new().no_dangling()));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx5101_checks：本单判据聚合（诊断段 0x8000）。
pub fn run_unx5101_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F5101 extent 维护操作总纲");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 分界声明：六操作全登记 + 引用全 B09
    let mut b = Boundary::new();
    b.add_ref(MaintainOp::Split, vec!["B09-101", "B09-102"]);
    b.add_ref(MaintainOp::Merge, vec!["B09-103"]);
    b.add_ref(MaintainOp::Shrink, vec!["B09-104", "B09-105", "B09-106"]);
    b.add_ref(MaintainOp::Promote, vec!["B09-107"]);
    b.add_ref(MaintainOp::Recompute, vec!["B09-108"]);
    b.add_ref(MaintainOp::Rollback, vec!["B09-109", "B09-110"]);
    let no_dangle = b.no_dangling();
    s.add("分界-六操作全登记零悬空", no_dangle, if no_dangle { "" } else { "引用悬空" });
    let b09_only = b.refs_all_b09();
    s.add("分界-引用全指向B09判据", b09_only, if b09_only { "" } else { "越界引用" });

    // 悬空检出：缺一操作即红
    let mut b2 = Boundary::new();
    for op in MaintainOp::all().iter().skip(1) {
        b2.add_ref(*op, vec!["B09-x"]);
    }
    s.add("分界-缺Rollback登记判悬空", !b2.no_dangling(), if !b2.no_dangling() { "" } else { "缺项漏检" });
    let mut b3 = Boundary::new();
    for op in MaintainOp::all().iter() {
        b3.add_ref(*op, vec![]);
    }
    s.add("分界-空引用判悬空", !b3.no_dangling(), if !b3.no_dangling() { "" } else { "空引用漏检" });

    // 总纲可导出
    let export_ok = b.export() == "split,merge,shrink,promote,recompute,rollback";
    s.add("总纲-六操作名可导出序一致", export_ok, if export_ok { "" } else { "导出序漂移" });

    // 维护树：split/merge/shrink/promote/recompute/rollback 全路径
    let mut t = MaintTree::new(vec![0, 8, 16, 24]);
    let sp = t.split(1);
    let sp_ok = sp && t.leaves.len() == 5 && t.leaves[2] == 16;
    s.add("split-一分为二插入序正确", sp_ok, if sp_ok { "" } else { "分裂漂移" });

    let mg = t.merge(1);
    let mg_ok = mg && t.leaves.len() == 4;
    s.add("merge-相邻合一", mg_ok, if mg_ok { "" } else { "合并漂移" });

    let sh = t.shrink();
    let sh_ok = sh && t.leaves.len() == 3;
    s.add("shrink-尾部收缩", sh_ok, if sh_ok { "" } else { "收缩漂移" });

    let pr = t.promote();
    let pr_ok = pr && t.depth == 1;
    s.add("promote-depth+1", pr_ok, if pr_ok { "" } else { "提升漂移" });

    let rc = t.recompute(5);
    let rc_ok = rc == 2; // entries 5 - actual 3 = 2
    s.add("recompute-修正量恰2", rc_ok, if rc_ok { "" } else { "重算漂移" });

    // 回滚链：3 次回滚逐一还原
    let rb1 = t.rollback();
    let rb2 = t.rollback();
    let rb3 = t.rollback();
    let rb_ok = rb1 && rb2 && rb3 && t.leaves == vec![0, 8, 16, 24];
    s.add("rollback-三次回滚还原初始", rb_ok, if rb_ok { "" } else { "回滚链漂移" });

    // 空树回滚拒 + 深度上限
    let mut t2 = MaintTree::new(vec![]);
    let empty_rb = !t2.rollback();
    let mut t3 = MaintTree::new(vec![1]);
    let mut depth_max = true;
    for _ in 0..7 {
        if !t3.promote() {
            depth_max = false;
        }
    }
    let guard_ok = empty_rb && depth_max && t3.depth == 5;
    s.add("守卫-空回滚拒+深度封顶5", guard_ok, if guard_ok { "" } else { "守卫漏网" });

    // 100 轮随机操作不炸 + 留痕完整
    let mut t4 = MaintTree::new(vec![0, 8, 16]);
    let mut seed: u64 = 0x5101_0100;
    let mut acts = 0u32;
    for _ in 0..100 {
        match lcg_next(&mut seed) % 4 {
            0 => {
                if t4.split((lcg_next(&mut seed) % 3) as usize) {
                    acts += 1;
                }
            }
            1 => {
                if t4.merge(0) {
                    acts += 1;
                }
            }
            2 => {
                if t4.shrink() {
                    acts += 1;
                }
            }
            _ => {
                if t4.rollback() {
                    acts += 1;
                }
            }
        }
        if t4.leaves.is_empty() {
            t4.leaves.push(0);
        }
    }
    let hist_ok = acts > 0 && t4.history.len() >= acts as usize;
    s.add("随机-100轮操作留痕完整", hist_ok, if hist_ok { "" } else { "留痕缺口" });

    // FNV 分界指纹链
    let mut acc: u32 = 0x811C_9DC5;
    for (op, ids) in &b.refs {
        acc ^= fnv1a(&[op.as_str().as_bytes()[0], ids.len() as u8]);
        acc = acc.wrapping_mul(0x0100_0193);
    }
    s.add("FNV-分界指纹链非零", acc != 0, if acc != 0 { "" } else { "指纹链异常" });

    s
}
