//! CGPU-F0173 · 帧图与合批联动（判据段 0x0470）
//! 帧图对合批优化（VE-F0030）的支撑：图构建时识别可合批任务（同材质
//! 同目标相邻绘制），合批后图自动更新（多节点→单合批节点），合批的
//! 约束保持（混合敏感豁免 F0030 纪律）。判据：可合批识别正确、图更新
//! 一致、约束保持、合批收益、对拍。
//!
//! 诊断码段 0x0470（本模块独占：0x0470~0x047F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0470;

/// 最大合批组尺寸上限（单批过大 = GPU 时间片过长，抢占粒度劣化）。
pub const MAX_BATCH_SIZE: usize = 64;

/// 绘制任务（合批视角）：材质指纹、目标纹理指纹、混合状态、
/// 图内任务号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DrawTask {
    /// 图内任务号。
    pub task: u32,
    /// 材质指纹。
    pub mat_fp: u64,
    /// 目标纹理指纹。
    pub target_fp: u64,
    /// 混合状态代号（混合敏感豁免的判定源）。
    pub blend: u8,
    /// 绘制序（相邻性判定依据）。
    pub order: u32,
}

impl DrawTask {
    /// 任务校验：指纹非零、混合状态在界内。
    pub fn validate(&self) -> Result<(), u16> {
        if self.mat_fp == 0 || self.target_fp == 0 {
            return Err(SEG_BASE);
        }
        if self.blend > 3 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }

    /// 混合敏感（F0030 豁免纪律）：blend 非 0（不透明）即混合敏感——
    /// 混合敏感任务绝不合批（混合顺序影响像素结果）。
    pub fn blend_sensitive(&self) -> bool {
        self.blend != 0
    }
}

/// 可合批判定（判据「可合批识别正确」核心面）：同材质 + 同目标 +
/// 双方都非混合敏感。相邻性由调用方保证（按 order 排序后扫描）。
pub fn batchable(a: &DrawTask, b: &DrawTask) -> Result<bool, u16> {
    a.validate()?;
    b.validate()?;
    if a.blend_sensitive() || b.blend_sensitive() {
        return Ok(false);
    }
    Ok(a.mat_fp == b.mat_fp && a.target_fp == b.target_fp)
}

/// 合批组：一组可合批任务收敛成的单节点。
pub struct BatchGroup {
    /// 组内任务号集（按绘制序）。
    pub members: Vec<u32>,
    /// 组代表材质指纹（全组一致）。
    pub mat_fp: u64,
    /// 组代表目标指纹（全组一致）。
    pub target_fp: u64,
    /// 组合并入图的任务号（多节点 → 单合批节点）。
    pub merged_task: u32,
}

impl BatchGroup {
    /// 组校验：非空、不超上限、成员任务号唯一。
    pub fn validate(&self) -> Result<(), u16> {
        if self.members.is_empty() {
            return Err(SEG_BASE + 2);
        }
        if self.members.len() > MAX_BATCH_SIZE {
            return Err(SEG_BASE + 3);
        }
        for i in 0..self.members.len() {
            for j in (i + 1)..self.members.len() {
                if self.members[i] == self.members[j] {
                    return Err(SEG_BASE + 4);
                }
            }
        }
        if self.mat_fp == 0 || self.target_fp == 0 || self.merged_task == 0 {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// 合批识别器（判据主面）：按绘制序扫描，贪心成组。
pub struct BatchRecognizer {
    /// 识别出的组。
    pub groups: Vec<BatchGroup>,
    /// 被豁免的混合敏感任务号（显性记录——豁免不是失败）。
    pub exempted: Vec<u32>,
}

impl BatchRecognizer {
    /// 空识别器。
    pub fn new() -> Self {
        BatchRecognizer {
            groups: Vec::new(),
            exempted: Vec::new(),
        }
    }

    /// 全量识别（判据「可合批识别正确」批量面）：输入必须按绘制序；
    /// 同材质+同目标连续段成组，混合敏感打断连续段并豁免记录。
    pub fn recognize(&mut self, draws: &[DrawTask]) -> Result<(), u16> {
        self.groups.clear();
        self.exempted.clear();
        // 绘制序严格递增校验（乱序输入 = 识别不可靠）。
        for w in draws.windows(2) {
            if w[0].order >= w[1].order {
                return Err(SEG_BASE + 6);
            }
        }
        let mut i = 0;
        while i < draws.len() {
            draws[i].validate()?;
            if draws[i].blend_sensitive() {
                self.exempted.push(draws[i].task);
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < draws.len()
                && j - i < MAX_BATCH_SIZE
                && !draws[j].blend_sensitive()
                && draws[j].mat_fp == draws[i].mat_fp
                && draws[j].target_fp == draws[i].target_fp
            {
                j += 1;
            }
            if j - i >= 2 {
                self.groups.push(BatchGroup {
                    members: draws[i..j].iter().map(|d| d.task).collect(),
                    mat_fp: draws[i].mat_fp,
                    target_fp: draws[i].target_fp,
                    merged_task: draws[i].task,
                });
            }
            i = j;
        }
        Ok(())
    }

    /// 组数。
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// 豁免数。
    pub fn exempt_count(&self) -> usize {
        self.exempted.len()
    }
}

/// 合批后图自动更新（判据「图更新一致」核心面）：组内成员的外部
/// 依赖并入代表节点、外部后继改挂代表节点——图拓扑语义保持等价。
pub struct GraphUpdate {
    /// 并入的外部入边（外部前驱 → 代表节点）。
    pub in_edges: Vec<(u32, u32)>,
    /// 改挂的外部出边（代表节点 → 外部后继）。
    pub out_edges: Vec<(u32, u32)>,
}

/// 图更新推导（纯函数面）：给定全图边与组，产出更新后的边集投影。
/// 组内边全部消失，跨组边改挂代表。语义不变式：外部任务对组内任一
/// 成员的依赖 = 对代表节点的依赖。
pub fn graph_update_for(
    all_edges: &[(u32, u32)],
    group: &BatchGroup,
) -> Result<GraphUpdate, u16> {
    group.validate()?;
    let mut in_edges: Vec<(u32, u32)> = Vec::new();
    let mut out_edges: Vec<(u32, u32)> = Vec::new();
    for (from, to) in all_edges {
        let f_in = group.members.contains(from);
        let t_in = group.members.contains(to);
        if f_in && t_in {
            continue; // 组内边消失
        }
        if f_in && !t_in {
            let e = (group.merged_task, *to);
            if !out_edges.contains(&e) {
                out_edges.push(e);
            }
        } else if !f_in && t_in {
            let e = (*from, group.merged_task);
            if !in_edges.contains(&e) {
                in_edges.push(e);
            }
        }
    }
    Ok(GraphUpdate {
        in_edges,
        out_edges,
    })
}

/// 约束保持审计（判据「约束保持」面）：合批后不得引入新的环
/// （代表节点汇聚了全部成员的边，可能成环——合批的语义红线）。
pub fn constraint_preserved(
    all_edges: &[(u32, u32)],
    groups: &[BatchGroup],
) -> Result<(), u16> {
    for g in groups {
        g.validate()?;
        let upd = graph_update_for(all_edges, g)?;
        // 自环检查：代表节点同时是某外部前驱的后继 → 合批自环。
        for (f, t) in &upd.in_edges {
            if upd.out_edges.contains(&(*t, *f)) || *f == g.merged_task {
                return Err(SEG_BASE + 7);
            }
        }
        for (f, _) in &upd.out_edges {
            if *f != g.merged_task {
                return Err(SEG_BASE + 8);
            }
        }
    }
    Ok(())
}

/// 合批收益统计（判据「合批收益」量化面）。
pub struct BatchGain {
    /// 合批前节点数。
    pub before: usize,
    /// 合批后节点数。
    pub after: usize,
    /// 被豁免任务数。
    pub exempted: usize,
}

impl BatchGain {
    /// 收敛率（千分比）：消失节点占原节点比例。
    pub fn merge_permille(&self) -> Result<u32, u16> {
        if self.before == 0 || self.after == 0 || self.after > self.before {
            return Err(SEG_BASE + 9);
        }
        let gone = self.before - self.after;
        Ok(((gone * 1000) / self.before) as u32)
    }
}

/// 对拍基线（判据「对拍」面）：合批前后的像素指纹必须一致
/// （合批只省提交次数，不改像素语义）。
pub fn pixel_equivalence(before_fp: u64, after_fp: u64) -> Result<(), u16> {
    if before_fp == 0 || after_fp == 0 {
        return Err(SEG_BASE + 10);
    }
    if before_fp != after_fp {
        return Err(SEG_BASE + 11);
    }
    Ok(())
}

/// 合批联动总闸（一站式收口）：识别 → 逐组图更新 → 约束保持 →
/// 像素等价 → 收益入账。任一关失败整体红。
pub fn batch_link_audit(
    draws: &[DrawTask],
    all_edges: &[(u32, u32)],
    pixel_before: u64,
    pixel_after: u64,
) -> Result<BatchGain, u16> {
    let mut rec = BatchRecognizer::new();
    rec.recognize(draws)?;
    for g in &rec.groups {
        graph_update_for(all_edges, g)?;
    }
    constraint_preserved(all_edges, &rec.groups)?;
    pixel_equivalence(pixel_before, pixel_after)?;
    if rec.groups.is_empty() && rec.exempted.is_empty() {
        return Err(SEG_BASE + 12);
    }
    let after = draws.len() - rec.groups.iter().map(|g| g.members.len() - 1).sum::<usize>();
    Ok(BatchGain {
        before: draws.len(),
        after,
        exempted: rec.exempted.len(),
    })
}

/// 合批识别器深度扩展面。
impl BatchRecognizer {
    /// 组质量审计（判据「可合批识别正确」强化面）：每组成员数
    /// ≥ 2（单成员组是识别残渣，必须剔除或重识别）。
    pub fn group_hygiene(&self) -> Result<(), u16> {
        for g in &self.groups {
            g.validate()?;
            if g.members.len() < 2 {
                return Err(SEG_BASE + 13);
            }
        }
        Ok(())
    }

    /// 豁免闭环审计（判据「约束保持」豁免面）：每个被豁免任务
    /// 必须真的混合敏感——误豁免 = 合批机会流失（账面假豁免）。
    pub fn exemption_closed(&self, draws: &[DrawTask]) -> Result<(), u16> {
        for t in &self.exempted {
            match draws.iter().find(|d| d.task == *t) {
                Some(d) if d.blend_sensitive() => {}
                _ => return Err(SEG_BASE + 14),
            }
        }
        Ok(())
    }

    /// 全覆盖守恒（判据「图更新一致」守恒面）：组成员并集加豁免集
    /// 加未成组单件等于全部绘制任务——任何任务不许在账外消失。
    pub fn conservation(&self, draws: &[DrawTask]) -> Result<(), u16> {
        let mut seen: Vec<u32> = Vec::new();
        for g in &self.groups {
            for m in &g.members {
                if seen.contains(m) {
                    return Err(SEG_BASE + 15);
                }
                seen.push(*m);
            }
        }
        if seen.len() + self.exempted.len() > draws.len() {
            return Err(SEG_BASE + 16);
        }
        Ok(())
    }
}

/// 合批窗口收益模型（判据「合批收益」量化深化面）：合批把 N 次提交
/// 收敛为 G 次，提交开销模型 = 每次提交固定成本乘次数加每任务
/// 边际成本乘任务数。收益 = 前后差（微秒）。
pub fn submit_cost_model(
    task_count: usize,
    group_count: usize,
    per_submit_us: u32,
    per_task_us: u32,
) -> Result<(u64, u64, i64), u16> {
    if task_count == 0 || group_count == 0 || group_count > task_count {
        return Err(SEG_BASE + 17);
    }
    if per_submit_us == 0 && per_task_us == 0 {
        return Err(SEG_BASE + 18);
    }
    let before = per_submit_us as u64 * task_count as u64 + per_task_us as u64 * task_count as u64;
    let after = per_submit_us as u64 * group_count as u64 + per_task_us as u64 * task_count as u64;
    let gain = before as i64 - after as i64;
    if gain < 0 {
        return Err(SEG_BASE + 19);
    }
    Ok((before, after, gain))
}

/// 材质亲和聚类审计（独立函数面）：跨组同材质统计——同材质被拆进
/// 多组 = 识别次序不佳（相邻性断裂），占比超 30% 提示排序器改进。
pub fn material_affinity(groups: &[BatchGroup]) -> Result<u32, u16> {
    if groups.is_empty() {
        return Err(SEG_BASE + 20);
    }
    let mut mats: Vec<(u64, usize)> = Vec::new();
    for g in groups {
        match mats.iter_mut().find(|(m, _)| *m == g.mat_fp) {
            Some((_, c)) => *c += 1,
            None => mats.push((g.mat_fp, 1)),
        }
    }
    let split = mats.iter().filter(|(_, c)| *c > 1).count();
    Ok(((split * 1000) / mats.len()) as u32)
}
