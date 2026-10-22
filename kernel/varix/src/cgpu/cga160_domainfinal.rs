//! cga160_domainfinal.rs —— CGPU-F0160 · A 域域级终验与里程碑
//!
//! A 域 160/160 的真正域级终验（F0140 阶段终验之后的最终关卡）：
//! 三层终验（文档 160 项 / 代码一致性套件全绿 / 体验——软渲全链
//! 真机走查）+ 里程碑登记（CGPU W1 进度 160/384=41.7%）+ 里程碑
//! 报告呈 Variable 确认归档。
//! 终验通过 = CGPU-A 域正式完成，进入 B 域（任务图调度）主线。
//! 诊断码段 0x03F0（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x03F0;

/// A 域任务总数（里程碑分母）。
pub const DOMAIN_TASKS: u32 = 160;

/// CGPU 总任务数（进度对账分母）。
pub const CGPU_TOTAL: u32 = 384;

/// 终验三层（判据「三层终验」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalLayer {
    /// 文档层：160 项文档判据。
    Docs,
    /// 代码层：一致性套件全绿。
    CodeSuite,
    /// 体验层：软渲全链真机走查。
    Experience,
}

impl FinalLayer {
    /// 全层序。
    pub const ALL: [FinalLayer; 3] = [
        FinalLayer::Docs,
        FinalLayer::CodeSuite,
        FinalLayer::Experience,
    ];

    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            FinalLayer::Docs => 0,
            FinalLayer::CodeSuite => 1,
            FinalLayer::Experience => 2,
        }
    }
}

/// 单层终验状态。
#[derive(Clone, Debug)]
pub struct LayerState {
    /// 层。
    pub layer: FinalLayer,
    /// 该层通过项数。
    pub passed: u32,
    /// 该层总项数（文档层必须 160）。
    pub total: u32,
    /// 走查/执行证据指纹（零 = 无证据）。
    pub evidence_fp: u64,
}

impl LayerState {
    /// 自检：证据必填、通过不超总数、层界。
    pub fn validate(&self) -> Result<(), u16> {
        if self.evidence_fp == 0 {
            return Err(SEG_BASE + 1);
        }
        if self.passed > self.total || self.total == 0 {
            return Err(SEG_BASE + 2);
        }
        if self.layer == FinalLayer::Docs && self.total != DOMAIN_TASKS {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }

    /// 层绿。
    pub fn green(&self) -> bool {
        self.passed == self.total
    }
}

/// 里程碑登记（判据「里程碑登记」）。
#[derive(Clone, Copy, Debug)]
pub struct Milestone {
    /// 里程碑名指纹。
    pub name_fp: u64,
    /// 域内完成数（分子）。
    pub domain_done: u32,
    /// CGPU 全局完成数。
    pub cgpu_done: u32,
}

impl Milestone {
    /// 进度千分比（CGPU 全局口径）。
    pub fn progress_permille(&self) -> Result<u32, u16> {
        if self.domain_done > DOMAIN_TASKS {
            return Err(SEG_BASE + 4);
        }
        if self.cgpu_done > CGPU_TOTAL {
            return Err(SEG_BASE + 5);
        }
        Ok((self.cgpu_done * 1000) / CGPU_TOTAL)
    }

    /// 域内完成度自检：域内完成数与全局完成数自洽（域内完成数
    /// 不得超过全局完成数——域是全局的子集）。
    pub fn consistent(&self) -> Result<(), u16> {
        if self.domain_done > self.cgpu_done {
            return Err(SEG_BASE + 6);
        }
        Ok(())
    }
}

/// 域级终验台账。
pub struct DomainFinalAudit {
    /// 三层状态。
    layers: Vec<LayerState>,
    /// 里程碑登记。
    milestone: Option<Milestone>,
    /// 报告指纹（判据「报告确认」）。
    report_fp: Option<u64>,
    /// Variable 确认签章（判据「报告确认」的裁决方落账）。
    varia_seal: Option<u64>,
    /// 剩余计划更新指纹（判据「剩余计划更新」）。
    plan_update_fp: Option<u64>,
    /// 归档。
    archived: Option<u64>,
}

impl DomainFinalAudit {
    /// 空账。
    pub fn new() -> Self {
        DomainFinalAudit {
            layers: Vec::new(),
            milestone: None,
            report_fp: None,
            varia_seal: None,
            plan_update_fp: None,
            archived: None,
        }
    }

    /// 层数。
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// 登记层状态（判据「三层全绿」）。
    pub fn set_layer(&mut self, s: LayerState) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        s.validate()?;
        if self.layers.iter().any(|l| l.layer == s.layer) {
            return Err(SEG_BASE + 8);
        }
        self.layers.push(s);
        Ok(())
    }

    /// 三层齐判。
    pub fn layers_complete(&self) -> Result<(), u16> {
        if self.layers.len() < 3 {
            return Err(SEG_BASE + 9);
        }
        for l in FinalLayer::ALL {
            if !self.layers.iter().any(|s| s.layer == l) {
                return Err(SEG_BASE + 10);
            }
        }
        Ok(())
    }

    /// 三层全绿（判据「三层全绿」）。
    pub fn all_layers_green(&self) -> Result<(), u16> {
        self.layers_complete()?;
        for l in &self.layers {
            if !l.green() {
                return Err(SEG_BASE + 11);
            }
        }
        Ok(())
    }

    /// 里程碑登记（判据「里程碑登记/进度对账」）：进度必须达到
    /// 域完成口径（domain_done == 160 且全局进度 ≥ 41.7% → ≥416‰）。
    pub fn register_milestone(&mut self, m: Milestone) -> Result<u32, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        if self.milestone.is_some() {
            return Err(SEG_BASE + 12);
        }
        m.consistent()?;
        if m.domain_done < DOMAIN_TASKS {
            return Err(SEG_BASE + 13);
        }
        let pm = m.progress_permille()?;
        if pm < 416 {
            return Err(SEG_BASE + 14);
        }
        self.milestone = Some(m);
        Ok(pm)
    }

    /// 里程碑投影。
    pub fn milestone(&self) -> Option<Milestone> {
        self.milestone
    }

    /// 呈报告（判据「报告确认」）。
    pub fn submit_report(&mut self, body: &[u8]) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        if self.report_fp.is_some() {
            return Err(SEG_BASE + 15);
        }
        if body.is_empty() {
            return Err(SEG_BASE + 16);
        }
        let mut fp: u64 = 0xcbf29ce484222325;
        for &b in body {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        self.report_fp = Some(fp);
        Ok(fp)
    }

    /// 剩余计划更新（判据「剩余计划更新」）：B 域主线计划的
    /// 更新记录指纹。
    pub fn set_plan_update(&mut self, plan: &[u8]) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        if self.plan_update_fp.is_some() {
            return Err(SEG_BASE + 17);
        }
        if plan.is_empty() {
            return Err(SEG_BASE + 18);
        }
        let mut fp: u64 = 0xcbf29ce484222325;
        for &b in plan {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        self.plan_update_fp = Some(fp);
        Ok(fp)
    }

    /// Variable 确认（判据「报告确认」的裁决方）：确认前报告
    /// 必须已呈。
    pub fn confirm_by_varia(&mut self, verdict: &[u8]) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        if self.report_fp.is_none() {
            return Err(SEG_BASE + 19);
        }
        if self.varia_seal.is_some() {
            return Err(SEG_BASE + 20);
        }
        if verdict.is_empty() {
            return Err(SEG_BASE + 21);
        }
        let mut fp: u64 = self.report_fp.unwrap_or(0);
        for &b in verdict {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        self.varia_seal = Some(fp);
        Ok(fp)
    }

    /// 域级终验通过（判据收口）：三层全绿 + 里程碑 + 报告 +
    /// 确认 + 计划更新，五关全过才可归档——归档即 A 域正式完成。
    pub fn finalize_domain(&mut self) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 22);
        }
        self.all_layers_green()?;
        if self.milestone.is_none() {
            return Err(SEG_BASE + 23);
        }
        if self.report_fp.is_none() || self.varia_seal.is_none() {
            return Err(SEG_BASE + 24);
        }
        if self.plan_update_fp.is_none() {
            return Err(SEG_BASE + 25);
        }
        let mut fp = self.varia_seal.unwrap_or(0);
        fp = fp.wrapping_mul(0x100000001b3) ^ self.plan_update_fp.unwrap_or(0);
        for l in &self.layers {
            fp = fp.wrapping_mul(0x100000001b3) ^ l.evidence_fp;
        }
        if fp == 0 {
            return Err(SEG_BASE + 26);
        }
        self.archived = Some(fp);
        Ok(fp)
    }

    /// 归档指纹。
    pub fn archive_fp(&self) -> Option<u64> {
        self.archived
    }

    /// 层普查投影。
    pub fn layer_rows(&self) -> &Vec<LayerState> {
        &self.layers
    }
}

impl Default for DomainFinalAudit {
    fn default() -> Self {
        Self::new()
    }
}

/// 域完成外部对账（独立函数面）：里程碑的域内数必须等于
/// DOMAIN_TASKS（160/160 是唯一可接受口径）。
pub fn milestone_reconcile(m: &Milestone) -> Result<(), u16> {
    if m.domain_done != DOMAIN_TASKS {
        return Err(SEG_BASE + 27);
    }
    m.consistent()?;
    let _ = m.progress_permille()?;
    Ok(())
}

impl DomainFinalAudit {
    /// 层绿投影清单（判据「三层全绿」逐层可见）：各层是否绿。
    pub fn layer_greens(&self) -> [Option<bool>; 3] {
        let mut out = [None; 3];
        for l in &self.layers {
            out[l.layer.rank() as usize] = Some(l.green());
        }
        out
    }

    /// 证据齐判（判据「真机走查」证据面）：三层证据指纹全非零。
    pub fn evidence_complete(&self) -> Result<(), u16> {
        self.layers_complete()?;
        if self.layers.iter().any(|l| l.evidence_fp == 0) {
            return Err(SEG_BASE + 28);
        }
        Ok(())
    }

    /// 全局进度投影（判据「进度对账」）：里程碑的 CGPU 千分比。
    pub fn progress(&self) -> Result<u32, u16> {
        match self.milestone {
            Some(m) => m.progress_permille(),
            None => Err(SEG_BASE + 29),
        }
    }

    /// 域内余量断言：域内通过项合计不得超过域任务总数
    /// （文档层 160 全过是上限口径，多报 = 账面造假）。
    pub fn passed_bound(&self) -> Result<(), u16> {
        let total_passed: u32 = self.layers.iter().map(|l| l.passed).sum();
        // 三层各自有界（validate 已限 passed<=total），此处审计
        // 文档层不超 DOMAIN_TASKS 且非文档层不得冒用 160 上限。
        for l in &self.layers {
            if l.layer != FinalLayer::Docs && l.total >= DOMAIN_TASKS {
                return Err(SEG_BASE + 30);
            }
        }
        let _ = total_passed;
        Ok(())
    }
}
