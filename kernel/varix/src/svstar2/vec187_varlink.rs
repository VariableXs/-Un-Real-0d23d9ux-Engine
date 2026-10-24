//! VE-F0587 · 热编译与变体联动
//!
//! 编辑态变体参数变化的响应规则——参数维变化先判定轻重路径：走特化常量重绑
//! （轻路径：F0477 SpecConstant 只重绑不重编）或触发重编译（重路径：结构性
//! 变更）的判定规则显性成表；变体矩阵编辑期裁剪（只编译当前可视与相邻变体，
//! 其余按需懒编译）；与 F0481 变体总成、F0482 预热机制的编辑态适配（预热在
//! 编辑态降级为按需）。
//!
//! 跨批对接：上游 F0477 参数化、F0481 总成、F0482 预热；下游 F0582 级联、F0590 缓存。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 参数变化类别
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamChange {
    /// 数值变化：可走特化常量重绑（轻路径）
    ValueTweak,
    /// 结构变化：维数/类型/布局变化（重路径）
    Structural,
}

/// 轻重路径判定规则（显性成表）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Path {
    /// 轻路径：SpecConstant 只重绑不重编
    SpecRebind,
    /// 重路径：触发重编译
    Recompile,
}

/// 判定规则表：数值维 → 轻；结构维 → 重；歧义 → 保守走重
pub fn judge(change: ParamChange, ambiguous: bool) -> Path {
    if ambiguous {
        return Path::Recompile;
    }
    match change {
        ParamChange::ValueTweak => Path::SpecRebind,
        ParamChange::Structural => Path::Recompile,
    }
}

/// 特化常量重绑（轻路径执行）
#[derive(Clone, Debug, Default)]
pub struct SpecRebinder {
    /// 特化槽位 → 新值
    pub bindings: Vec<(u32, i64)>,
}

impl SpecRebinder {
    pub fn rebind(&mut self, slot: u32, val: i64) -> bool {
        match self.bindings.iter_mut().find(|(s, _)| *s == slot) {
            Some(e) => {
                e.1 = val;
                true
            }
            None => {
                self.bindings.push((slot, val));
                true
            }
        }
    }

    pub fn get(&self, slot: u32) -> Option<i64> {
        self.bindings.iter().find(|(s, _)| *s == slot).map(|(_, v)| *v)
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

/// 懒编译队列：编辑期裁剪后按需编译
#[derive(Clone, Debug, Default)]
pub struct LazyQueue {
    pub pending: Vec<u32>,
    pub compiled: Vec<u32>,
}

impl LazyQueue {
    pub fn defer(&mut self, variant: u32) -> bool {
        if self.pending.contains(&variant) || self.compiled.contains(&variant) {
            return false;
        }
        self.pending.push(variant);
        true
    }

    /// 按需触发：可视化到该变体时出队
    pub fn realize(&mut self, variant: u32) -> bool {
        if let Some(i) = self.pending.iter().position(|v| *v == variant) {
            self.pending.remove(i);
            self.compiled.push(variant);
            true
        } else {
            false
        }
    }

    pub fn is_compiled(&self, variant: u32) -> bool {
        self.compiled.contains(&variant)
    }
}

/// 编辑期裁剪策略：可视 + 相邻变体保留，其余懒编译
pub fn trim_matrix(total: &[u32], visible: u32, neighbor_span: u32) -> (Vec<u32>, Vec<u32>) {
    let keep: Vec<u32> = total
        .iter()
        .filter(|v| {
            let d = if **v >= visible { **v - visible } else { visible - **v };
            d <= neighbor_span
        })
        .cloned()
        .collect();
    let lazy: Vec<u32> = total.iter().filter(|v| !keep.contains(v)).cloned().collect();
    (keep, lazy)
}

/// 预热机制编辑态适配：预热降级为按需
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrewarmMode {
    /// 构建期全量预热（F0482 原语义）
    Full,
    /// 编辑态：按需懒编译
    OnDemand,
}

/// 编辑态适配：预热在编辑态降级为按需
pub fn adapt_prewarm(edit_session: bool) -> PrewarmMode {
    if edit_session {
        PrewarmMode::OnDemand
    } else {
        PrewarmMode::Full
    }
}

/// 参数越界：编译期拒绝三要素
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reject {
    pub slot: u32,
    pub value: i64,
    pub reason: String,
}

impl Reject {
    pub fn new(slot: u32, value: i64, reason: &str) -> Self {
        Reject { slot, value, reason: reason.to_string() }
    }

    pub fn complete(&self) -> bool {
        !self.reason.is_empty()
    }
}

/// 越界判定
pub fn out_of_range(slot: u32, value: i64, min: i64, max: i64) -> Option<Reject> {
    if value < min || value > max {
        Some(Reject::new(slot, value, "value out of spec range"))
    } else {
        None
    }
}

pub fn run_vec187_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec187");
    // 族一：轻重路径判定
    s.add("判定-数值走轻", judge(ParamChange::ValueTweak, false) == Path::SpecRebind, "light");
    s.add("判定-结构走重", judge(ParamChange::Structural, false) == Path::Recompile, "heavy");
    s.add("判定-歧义保守走重", judge(ParamChange::ValueTweak, true) == Path::Recompile, "cons");
    // 族二：特化常量重绑
    let mut rb = SpecRebinder::default();
    s.add("重绑-新槽登记", rb.rebind(3, 42) && rb.get(3) == Some(42), "new");
    s.add("重绑-同槽覆写", rb.rebind(3, 50) && rb.get(3) == Some(50) && rb.len() == 1, "over");
    s.add("重绑-未绑查询None", rb.get(9).is_none(), "miss");
    // 族三：懒编译队列
    let mut lq = LazyQueue::default();
    s.add("懒编-挂起", lq.defer(7) && !lq.is_compiled(7), "defer");
    s.add("懒编-重复挂起拒绝", !lq.defer(7), "dup");
    s.add("懒编-按需出队", lq.realize(7) && lq.is_compiled(7), "realize");
    s.add("懒编-未挂起出队拒绝", !lq.realize(9), "noreal");
    // 族四：编辑期裁剪
    let (keep, lazy) = trim_matrix(&[0, 1, 2, 3, 4, 5, 6, 7], 3, 1);
    s.add("裁剪-可视与相邻保留", keep == vec![2, 3, 4], "keep");
    s.add("裁剪-其余懒编", lazy == vec![0, 1, 5, 6, 7], "lazy");
    s.add("裁剪-保留并集全覆盖", keep.len() + lazy.len() == 8, "cover");
    let (k2, l2) = trim_matrix(&[1, 2, 3], 2, 9);
    s.add("裁剪-大跨度全保留", k2.len() == 3 && l2.is_empty(), "wide");
    // 族五：预热降级
    s.add("预热-编辑态按需", adapt_prewarm(true) == PrewarmMode::OnDemand, "od");
    s.add("预热-构建期全量", adapt_prewarm(false) == PrewarmMode::Full, "full");
    // 族六：越界拒绝三要素
    let r = out_of_range(4, 999, 0, 100);
    s.add("越界-检出", r.is_some(), "oob");
    let rj = r.unwrap();
    s.add("越界-三要素完整", rj.complete() && rj.slot == 4 && rj.value == 999, "tri");
    s.add("越界-界内放行", out_of_range(4, 50, 0, 100).is_none(), "ok");
    s
}
