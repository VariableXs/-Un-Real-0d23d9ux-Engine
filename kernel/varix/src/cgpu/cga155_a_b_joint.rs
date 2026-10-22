//! cga155_a_b_joint.rs —— CGPU-F0155 · A 域与 CGPU-B 联调记录
//!
//! A 域算子经 B 域任务图调度的联合调试记录：B 域首批调度场景
//! （A 域算子作为任务节点）的联测——调度正确性、确定性保持、
//! 开销测量；联调发现问题的处置记录与归档。
//! 设计立场：联调的本质是「两本账对上」——A 域的算子语义账
//! 与 B 域的调度序账必须指向同一次执行结果。
//! 诊断码段 0x03A0（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x03A0;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 联测场景。
#[derive(Clone, Debug)]
pub struct JointScene {
    /// 场景号。
    pub scene: u32,
    /// 参与的 A 域算子号集（指纹折算）。
    pub op_fps: Vec<u64>,
    /// B 域任务图场景指纹。
    pub graph_fp: u64,
    /// 期望输出指纹。
    pub expect_fp: u64,
}

impl JointScene {
    /// 自检：算子非空、图指纹与期望指纹非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.op_fps.is_empty() {
            return Err(SEG_BASE + 1);
        }
        if self.graph_fp == 0 || self.expect_fp == 0 {
            return Err(SEG_BASE + 2);
        }
        for f in &self.op_fps {
            if *f == 0 {
                return Err(SEG_BASE + 3);
            }
        }
        Ok(())
    }
}

/// 单次联测结果。
#[derive(Clone, Copy, Debug)]
pub struct JointRun {
    /// 场景号。
    pub scene: u32,
    /// 调度正确（执行序满足依赖约束）。
    pub sched_ok: bool,
    /// 确定性保持（同图同输入两次执行输出一致）。
    pub det_ok: bool,
    /// 输出与期望一致。
    pub out_ok: bool,
    /// 调度开销（微秒，定点）。
    pub sched_overhead_us: u64,
}

impl JointRun {
    /// 全绿判定。
    pub fn green(&self) -> bool {
        self.sched_ok && self.det_ok && self.out_ok
    }
}

/// 问题处置记录。
#[derive(Clone, Debug)]
pub struct IssueRow {
    /// 缺陷号。
    pub issue: u32,
    /// 场景号。
    pub scene: u32,
    /// 归属：0=A域算子 1=B域调度 2=接口契约 3=环境。
    pub owner: u8,
    /// 处置态：false=未清 true=已清。
    pub resolved: bool,
    /// 处置记录指纹。
    pub fix_fp: u64,
}

/// 联调台账。
pub struct JointDebugLedger {
    /// 场景册。
    scenes: Vec<JointScene>,
    /// 联测行。
    runs: Vec<JointRun>,
    /// 问题册。
    issues: Vec<IssueRow>,
    /// 开销预算（微秒）：单场景调度开销上限。
    overhead_budget_us: u64,
    /// 归档。
    archived: Option<u64>,
}

impl JointDebugLedger {
    /// 建账：开销预算必须给出（无预算的开销测量是摆设）。
    pub fn new(overhead_budget_us: u64) -> Result<Self, u16> {
        if overhead_budget_us == 0 {
            return Err(SEG_BASE + 4);
        }
        Ok(JointDebugLedger {
            scenes: Vec::new(),
            runs: Vec::new(),
            issues: Vec::new(),
            overhead_budget_us,
            archived: None,
        })
    }

    /// 场景数。
    pub fn scene_count(&self) -> usize {
        self.scenes.len()
    }

    /// 联测行数。
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// 登记场景（判据「联测场景」）。
    pub fn add_scene(&mut self, s: JointScene) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 5);
        }
        s.validate()?;
        if self.scenes.iter().any(|e| e.scene == s.scene) {
            return Err(SEG_BASE + 6);
        }
        self.scenes.push(s);
        Ok(())
    }

    /// 记一次联测（判据「确定性保持/开销」）：
    /// 场景须在册；开销超预算记超支但不拦账（真数据优先）。
    pub fn record(&mut self, r: JointRun) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 5);
        }
        if !self.scenes.iter().any(|s| s.scene == r.scene) {
            return Err(SEG_BASE + 7);
        }
        self.runs.push(r);
        Ok(())
    }

    /// 场景绿判（判据「调度正确性」）：场景全部联测行全绿。
    pub fn scene_green(&self, scene: u32) -> Result<(), u16> {
        let mut found = false;
        let mut all_green = true;
        for r in &self.runs {
            if r.scene == scene {
                found = true;
                if !r.green() {
                    all_green = false;
                }
            }
        }
        if !found {
            return Err(SEG_BASE + 8);
        }
        if !all_green {
            return Err(SEG_BASE + 9);
        }
        Ok(())
    }

    /// 确定性全绿（判据「确定性保持」域级）：所有行 det_ok。
    pub fn determinism_green(&self) -> Result<(), u16> {
        if self.runs.is_empty() {
            return Err(SEG_BASE + 10);
        }
        if self.runs.iter().all(|r| r.det_ok) {
            Ok(())
        } else {
            Err(SEG_BASE + 11)
        }
    }

    /// 开销统计（判据「开销」）：平均调度开销（微秒）与超支行数。
    pub fn overhead_stats(&self) -> (Option<u64>, usize) {
        if self.runs.is_empty() {
            return (None, 0);
        }
        let sum: u64 = self.runs.iter().map(|r| r.sched_overhead_us).sum();
        let avg = sum / self.runs.len() as u64;
        let over = self
            .runs
            .iter()
            .filter(|r| r.sched_overhead_us > self.overhead_budget_us)
            .count();
        (Some(avg), over)
    }

    /// 开销预算合规（判据「开销」闭环）：平均不超预算且超支行
    /// 不超过总行数的 10%（偶发尖峰可容忍，趋势性超支不行）。
    pub fn overhead_green(&self) -> Result<(), u16> {
        let (avg, over) = self.overhead_stats();
        let a = match avg {
            Some(a) => a,
            None => return Err(SEG_BASE + 12),
        };
        if a > self.overhead_budget_us {
            return Err(SEG_BASE + 13);
        }
        if over * 10 > self.runs.len() {
            return Err(SEG_BASE + 14);
        }
        Ok(())
    }

    /// 立案（判据「问题处置」）：归属闭集、记录指纹必填。
    pub fn raise_issue(&mut self, issue: u32, scene: u32, owner: u8) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 5);
        }
        if owner > 3 {
            return Err(SEG_BASE + 15);
        }
        if !self.scenes.iter().any(|s| s.scene == scene) {
            return Err(SEG_BASE + 7);
        }
        if self.issues.iter().any(|i| i.issue == issue) {
            return Err(SEG_BASE + 16);
        }
        self.issues.push(IssueRow {
            issue,
            scene,
            owner,
            resolved: false,
            fix_fp: 0,
        });
        Ok(())
    }

    /// 销案：处置记录指纹必填（空记录 = 假销案）。
    pub fn resolve_issue(&mut self, issue: u32, fix: &[u8]) -> Result<(), u16> {
        let row = self
            .issues
            .iter_mut()
            .find(|i| i.issue == issue)
            .ok_or(SEG_BASE + 17)?;
        if fix.is_empty() {
            return Err(SEG_BASE + 18);
        }
        row.resolved = true;
        row.fix_fp = fnv1a64(fix);
        Ok(())
    }

    /// 归属普查：四归属各多少条。
    pub fn owner_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for i in &self.issues {
            out[i.owner as usize] += 1;
        }
        out
    }

    /// 归档前置总检（判据「记录归档」）：场景齐测 + 全绿 +
    /// 问题清零 + 开销合规，四关全过才许归档。
    pub fn pre_archive(&mut self, canon: &[u8]) -> Result<u64, u16> {
        if self.scenes.is_empty() || self.runs.is_empty() {
            return Err(SEG_BASE + 19);
        }
        for s in &self.scenes {
            self.scene_green(s.scene)?;
        }
        self.determinism_green()?;
        self.overhead_green()?;
        if self.issues.iter().any(|i| !i.resolved) {
            return Err(SEG_BASE + 20);
        }
        let mut fp: u64 = 0xcbf29ce484222325;
        for s in &self.scenes {
            fp = fp.wrapping_mul(0x100000001b3) ^ s.graph_fp;
        }
        for r in &self.runs {
            fp = fp.wrapping_mul(0x100000001b3) ^ (r.sched_overhead_us);
        }
        for &b in canon {
            fp = fp.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
        self.archived = Some(fp);
        Ok(fp)
    }

    /// 归档指纹。
    pub fn archive_fp(&self) -> Option<u64> {
        self.archived
    }
}

impl JointDebugLedger {
    /// 确定性双跑审计（判据「确定性保持」强化面）：同场景至少
    /// 两次联测且全部 det_ok——单次跑通不能证明确定性。
    pub fn determinism_double_run(&self) -> Result<(), u16> {
        let mut scenes_seen: Vec<u32> = Vec::new();
        for r in &self.runs {
            if !scenes_seen.contains(&r.scene) {
                scenes_seen.push(r.scene);
            }
        }
        for s in scenes_seen {
            let n = self.runs.iter().filter(|r| r.scene == s).count();
            if n < 2 {
                return Err(SEG_BASE + 21);
            }
        }
        self.determinism_green()
    }

    /// 场景算子覆盖普查：全部场景算子指纹的折并（联调覆盖面）。
    pub fn op_coverage_fold(&self) -> u64 {
        self.scenes
            .iter()
            .flat_map(|s| s.op_fps.iter().copied())
            .fold(0u64, |a, f| a ^ f)
    }

    /// 未测场景清单（判据「联测场景」闭环）。
    pub fn untested_scenes(&self) -> Vec<u32> {
        self.scenes
            .iter()
            .filter(|s| !self.runs.iter().any(|r| r.scene == s.scene))
            .map(|s| s.scene)
            .collect()
    }
}
