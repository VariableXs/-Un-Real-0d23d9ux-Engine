//! VE-F0626 混合隔离组语义实现（VE-D 域 · 目标 340 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0626`
//!
//! **判据（锚点原文逐条）**：
//! - **三步执行序**：子树离屏合成 → 组 alpha 乘入 → 组以混合模式参与外部合成；
//! - **缓冲池化**：隔离组缓冲池按需取用、尺寸按包围盒最小化、用毕归还；
//! - **四条件内联**：不透明度/混合模式/效果链/蒙版逐条判定（树属性变更点内联）；
//! - **嵌套闭合**：内层先合成闭合再参与外层（递归闭合语义）；
//! - **判据**：缓冲池耗尽→直通分配加告警；嵌套过深→深度告警。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 隔离判定四条件（F0605 清单的代码化）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsoCond {
    /// 不透明度 < 1
    pub translucent: bool,
    /// 混合模式 ≠ normal
    pub blend_nonnormal: bool,
    /// 效果链非空
    pub has_effects: bool,
    /// 蒙版存在
    pub has_mask: bool,
}

impl IsoCond {
    pub const NONE: IsoCond = IsoCond {
        translucent: false,
        blend_nonnormal: false,
        has_effects: false,
        has_mask: false,
    };

    /// 内联判定：任一条件成立即需隔离（O(1)，树属性变更点内联完成）
    pub fn needs_isolation(self) -> bool {
        self.translucent || self.blend_nonnormal || self.has_effects || self.has_mask
    }

    /// 命中条件数（诊断用）
    pub fn hit_count(self) -> u32 {
        self.translucent as u32
            + self.blend_nonnormal as u32
            + self.has_effects as u32
            + self.has_mask as u32
    }
}

/// 三步执行序（F0626 锚点定死的执行顺序）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsoStep {
    /// ① 子树离屏合成为组纹理
    OffscreenComposite,
    /// ② 组 alpha 乘入
    GroupAlphaMul,
    /// ③ 组整体以混合模式参与外部合成
    BlendOntoParent,
}

pub const STEP_COUNT: usize = 3;

/// 完整执行序（顺序不可变）
pub fn step_sequence() -> [IsoStep; STEP_COUNT] {
    [IsoStep::OffscreenComposite, IsoStep::GroupAlphaMul, IsoStep::BlendOntoParent]
}

/// 隔离组执行计划
#[derive(Clone, Debug, PartialEq)]
pub struct IsoGroupPlan {
    pub node_id: u32,
    pub cond: IsoCond,
    /// 组包围盒（离屏缓冲尺寸依据，最小化）
    pub bbox: (u32, u32, u32, u32),
    /// 组 alpha（0..1）
    pub group_alpha: f32,
    /// 嵌套深度（根=1）
    pub depth: u32,
}

impl IsoGroupPlan {
    pub fn new(node_id: u32, cond: IsoCond, bbox: (u32, u32, u32, u32), group_alpha: f32) -> IsoGroupPlan {
        IsoGroupPlan { node_id, cond, bbox, group_alpha, depth: 1 }
    }

    /// 离屏缓冲尺寸 = 包围盒尺寸（最小化，不取整画布）
    pub fn buf_size(&self) -> (u32, u32) {
        (self.bbox.2.saturating_sub(self.bbox.0), self.bbox.3.saturating_sub(self.bbox.1))
    }

    /// 嵌套过深告警阈值
    pub const DEPTH_WARN: u32 = 16;

    pub fn depth_warn(&self) -> bool {
        self.depth >= IsoGroupPlan::DEPTH_WARN
    }
}

/// 隔离组缓冲池：按需取用、尺寸最小化匹配、用毕归还
#[derive(Clone, Debug, Default)]
pub struct BufPool {
    free: Vec<((u32, u32), u32)>,
    acquired: u32,
    /// 池耗尽直通分配次数（告警账）
    pub direct_allocs: u32,
    /// 池命中次数
    pub pool_hits: u32,
}

/// 池容量上限（超出即拒绝入池，防内存无界）
pub const POOL_CAP: u32 = 64;

impl BufPool {
    pub fn new() -> BufPool {
        BufPool::default()
    }

    /// 取用：优先精确匹配，其次面积 ≥ 需求的最小者（最小化浪费）
    pub fn acquire(&mut self, need: (u32, u32)) -> ((u32, u32), bool) {
        // 精确命中
        if let Some(i) = self.free.iter().position(|&(sz, _)| sz == need) {
            self.free.remove(i);
            self.acquired += 1;
            self.pool_hits += 1;
            return (need, true);
        }
        // 面积覆盖命中（取面积最小覆盖者）
        let area = |s: (u32, u32)| s.0 as u64 * s.1 as u64;
        let need_area = area(need);
        let mut best: Option<usize> = None;
        for (i, &(sz, _)) in self.free.iter().enumerate() {
            if sz.0 >= need.0 && sz.1 >= need.1 && area(sz) >= need_area {
                best = match best {
                    None => Some(i),
                    Some(b) => {
                        if area(self.free[i].0) < area(self.free[b].0) {
                            Some(i)
                        } else {
                            Some(b)
                        }
                    }
                };
            }
        }
        if let Some(i) = best {
            let sz = self.free.remove(i).0;
            self.acquired += 1;
            self.pool_hits += 1;
            return (sz, true);
        }
        // 池空 → 直通分配加告警（降级矩阵第 1 条）
        self.direct_allocs += 1;
        self.acquired += 1;
        (need, false)
    }

    /// 归还：容量内入池
    pub fn release(&mut self, sz: (u32, u32)) -> bool {
        if (self.free.len() as u32) >= POOL_CAP {
            return false;
        }
        self.free.push((sz, 1));
        true
    }

    pub fn free_len(&self) -> usize {
        self.free.len()
    }
}

/// 嵌套闭合执行器：内层先合成闭合再参与外层
#[derive(Clone, Debug, PartialEq)]
pub struct NestCloser {
    /// 已闭合层数
    pub closed: u32,
    /// 最大栈深
    pub max_stack: u32,
}

impl NestCloser {
    pub fn new() -> NestCloser {
        NestCloser { closed: 0, max_stack: 0 }
    }

    /// 进入一层嵌套（返回当前深度）
    pub fn enter(&mut self) -> u32 {
        self.max_stack += 1;
        self.max_stack
    }

    /// 闭合一层（LIFO：内层先闭合）
    pub fn close(&mut self) -> u32 {
        if self.max_stack > 0 {
            self.max_stack -= 1;
            self.closed += 1;
        }
        self.closed
    }

    /// 嵌套闭合一致性：全部 enter 必须被 close 对冲
    pub fn balanced(&self) -> bool {
        self.max_stack == 0
    }
}

pub fn run_vec226_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec226");
    // 族一：四条件内联判定
    s.add("隔离-无条件不隔离", !IsoCond::NONE.needs_isolation(), "clean pass");
    let c1 = IsoCond { translucent: true, ..IsoCond::NONE };
    s.add("隔离-透明触发", c1.needs_isolation(), "opacity");
    let c2 = IsoCond { blend_nonnormal: true, ..IsoCond::NONE };
    s.add("隔离-混合触发", c2.needs_isolation(), "blend");
    let c3 = IsoCond { has_effects: true, ..IsoCond::NONE };
    s.add("隔离-效果触发", c3.needs_isolation(), "effects");
    let c4 = IsoCond { has_mask: true, ..IsoCond::NONE };
    s.add("隔离-蒙版触发", c4.needs_isolation(), "mask");
    s.add("隔离-计数", c1.hit_count() == 1 && IsoCond::NONE.hit_count() == 0, "hit count");
    // 族二：三步执行序
    let seq = step_sequence();
    s.add("执行序-步数", seq.len() == STEP_COUNT, "3 steps");
    s.add("执行序-顺序", seq[0] == IsoStep::OffscreenComposite
        && seq[1] == IsoStep::GroupAlphaMul
        && seq[2] == IsoStep::BlendOntoParent, "order fixed");
    // 族三：计划与缓冲尺寸
    let plan = IsoGroupPlan::new(7, c1, (10, 20, 50, 70), 0.5);
    s.add("计划-缓冲尺寸", plan.buf_size() == (40, 50), "bbox minimal");
    s.add("计划-深度正常", !plan.depth_warn(), "depth ok");
    let deep = IsoGroupPlan { depth: IsoGroupPlan::DEPTH_WARN, ..plan.clone() };
    s.add("计划-深度告警", deep.depth_warn(), "depth warn");
    // 族四：缓冲池
    let mut pool = BufPool::new();
    let (got, from_pool) = pool.acquire((40, 50));
    s.add("池-首取直通", got == (40, 50) && !from_pool && pool.direct_allocs == 1, "first direct");
    s.add("池-归还", pool.release((40, 50)) && pool.free_len() == 1, "release");
    let (_, hit) = pool.acquire((40, 50));
    s.add("池-精确命中", hit && pool.pool_hits == 1 && pool.direct_allocs == 1, "exact hit");
    pool.release((40, 50));
    let (g2, h2) = pool.acquire((30, 30));
    s.add("池-覆盖命中", h2 && g2 == (40, 50), "covering hit (minimal)");
    pool.release(g2);
    let (g3, h3) = pool.acquire((100, 100));
    s.add("池-过小直通", !h3 && g3 == (100, 100), "too small → direct");
    // 族五：池容量上限
    let mut pool2 = BufPool::new();
    let mut all_released = true;
    for i in 0..(POOL_CAP as usize + 4) {
        if !pool2.release((i as u32 + 1, 1)) {
            // 超容量后拒绝归还
            all_released = i < POOL_CAP as usize;
            break;
        }
    }
    s.add("池-容量上限拒还", !all_released, "cap reject");
    s.add("池-上限精确", pool2.free_len() == POOL_CAP as usize, "cap exact");
    // 族六：嵌套闭合
    let mut nc = NestCloser::new();
    nc.enter();
    nc.enter();
    s.add("嵌套-栈深", nc.max_stack == 2, "stack 2");
    nc.close();
    s.add("嵌套-LIFO闭合", nc.max_stack == 1 && nc.closed == 1, "inner first");
    nc.close();
    s.add("嵌套-平衡", nc.balanced() && nc.closed == 2, "balanced");
    nc.close();
    s.add("嵌套-过闭无害", nc.closed == 2 && nc.balanced(), "over-close safe");
    // 族七：降级矩阵
    s.add("降级-直通告警", pool.direct_allocs >= 1, "direct alloc warned");
    s.add("降级-深度阈值", IsoGroupPlan::DEPTH_WARN == 16, "threshold 16");
    s
}
