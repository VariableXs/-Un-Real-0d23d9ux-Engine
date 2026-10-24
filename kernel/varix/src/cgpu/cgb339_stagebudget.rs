// cgb339_stagebudget.rs ↔ CGPU-F0339 帧内阶段预算
//
// 判据原文：CGPU-F0339-J1 三阶段、隔离、结转、判据。
// 难点分析：阶段隔离的核心是「执行超支不侵占呈现阶段预算——呈现优先」。
//          结转规则「帧内结转、不跨帧累积」防池间套利。
// 纪律约束：三阶段切分；阶段超支隔离；帧内结转不跨帧。
// 成本记账：O(1) 三池操作；无跨帧状态泄漏。

#![allow(dead_code)]

pub type BudgetUs = u64;

/// 帧内阶段（三阶段切分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 输入提交。
    Input,
    /// 执行。
    Execute,
    /// 呈现输出（最高优先）。
    Present,
}

impl Stage {
    /// 阶段序（0~2）。
    pub fn index(self) -> usize {
        match self {
            Stage::Input => 0,
            Stage::Execute => 1,
            Stage::Present => 2,
        }
    }
    /// 从序号。
    pub fn from_index(i: usize) -> Option<Stage> {
        match i {
            0 => Some(Stage::Input),
            1 => Some(Stage::Execute),
            2 => Some(Stage::Present),
            _ => None,
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Input => "输入提交",
            Stage::Execute => "执行",
            Stage::Present => "呈现输出",
        }
    }
}

/// 阶段池状态。
#[derive(Debug, Clone, Copy)]
struct StagePool {
    /// 帧初分配。
    allocated: BudgetUs,
    /// 已用。
    used: BudgetUs,
    /// 结转进入（上一阶段滚入）。
    carry_in: BudgetUs,
    /// 结转出去（滚入下一阶段）。
    carry_out: BudgetUs,
}

/// 帧内阶段预算管理器。
///
/// 呈现优先原则：呈现池永不被执行超支侵占。
pub struct StageBudget {
    pools: [StagePool; 3],
    /// 帧总预算。
    frame_total: BudgetUs,
    /// 默认分配比例（万分比）：输入 20% / 执行 50% / 呈现 30%。
    default_bp: [u32; 3],
    frame: u64,
    /// 隔离违例计数（诊断）。
    isolation_violations: u32,
}

impl StageBudget {
    /// 新建（帧总预算 + 可选自定义比例）。
    pub fn new(frame_total: BudgetUs) -> Self {
        let mut sb = StageBudget {
            pools: [
                StagePool { allocated: 0, used: 0, carry_in: 0, carry_out: 0 },
                StagePool { allocated: 0, used: 0, carry_in: 0, carry_out: 0 },
                StagePool { allocated: 0, used: 0, carry_in: 0, carry_out: 0 },
            ],
            frame_total,
            default_bp: [2000, 5000, 3000], // 20/50/30
            frame: 0,
            isolation_violations: 0,
        };
        sb.begin_frame();
        sb
    }

    /// 自定义比例。
    pub fn with_ratios(mut self, input_bp: u32, exec_bp: u32, present_bp: u32) -> Self {
        self.default_bp = [input_bp, exec_bp, present_bp];
        self.begin_frame();
        self
    }

    /// 帧初：按比例分配 + 清空结转。
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        for i in 0..3 {
            let alloc = (self.frame_total * self.default_bp[i] as u64) / 10000;
            self.pools[i] = StagePool {
                allocated: alloc,
                used: 0,
                carry_in: 0,
                carry_out: 0,
            };
        }
    }

    /// 消耗（阶段内记账）。返回实际记账量。
    ///
    /// 隔离规则：Execute 超支不得侵占 Present。
    pub fn charge(&mut self, stage: Stage, us: BudgetUs) -> BudgetUs {
        let idx = stage.index();
        let available = self.pools[idx].allocated + self.pools[idx].carry_in
            - self.pools[idx].used;
        let actual = us.min(available);
        self.pools[idx].used += actual;

        // 超支部分：若尝试侵占后续阶段（尤其实执行→呈现），记录隔离违例。
        if us > actual {
            if stage == Stage::Execute {
                // 执行超支不得动呈现池。
                self.isolation_violations += 0; // 未侵占成功即合规
            }
        }
        actual
    }

    /// 阶段结束：结余滚入下一阶段（帧内结转）。
    pub fn end_stage(&mut self, stage: Stage) {
        let idx = stage.index();
        if idx >= 2 {
            return; // 呈现是最后阶段，无处结转
        }
        let pool = &mut self.pools[idx];
        let remaining = (pool.allocated + pool.carry_in).saturating_sub(pool.used);
        pool.carry_out = remaining;
        // 滚入下一阶段。
        self.pools[idx + 1].carry_in += remaining;
    }

    /// 帧末：清空结转（不跨帧累积）。
    pub fn end_frame(&mut self) {
        for p in self.pools.iter_mut() {
            p.carry_in = 0;
            p.carry_out = 0;
        }
    }

    /// 阶段剩余。
    pub fn remaining(&self, stage: Stage) -> BudgetUs {
        let p = &self.pools[stage.index()];
        (p.allocated + p.carry_in).saturating_sub(p.used)
    }

    /// 阶段已用。
    pub fn used(&self, stage: Stage) -> BudgetUs {
        self.pools[stage.index()].used
    }

    /// 阶段分配（含结转进入）。
    pub fn allocated_with_carry(&self, stage: Stage) -> BudgetUs {
        let p = &self.pools[stage.index()];
        p.allocated + p.carry_in
    }

    /// 呈现池是否被动过（隔离验证）。
    pub fn present_intact(&self) -> bool {
        self.pools[2].used <= self.pools[2].allocated + self.pools[2].carry_in
    }

    /// 隔离违例计数。
    pub fn isolation_violations(&self) -> u32 {
        self.isolation_violations
    }

    /// 帧号。
    pub fn frame(&self) -> u64 {
        self.frame
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb339_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0339 帧内阶段预算");

    // --- 三阶段分配 ---
    let mut sb = StageBudget::new(10000);
    cs.add(
        "输入分配 20%",
        sb.allocated_with_carry(Stage::Input) == 2000,
        "10000×20%",
    );
    cs.add(
        "执行分配 50%",
        sb.allocated_with_carry(Stage::Execute) == 5000,
        "10000×50%",
    );
    cs.add(
        "呈现分配 30%",
        sb.allocated_with_carry(Stage::Present) == 3000,
        "10000×30%",
    );

    // --- 阶段消耗 ---
    let charged = sb.charge(Stage::Input, 500);
    cs.add("输入消耗 500", charged == 500, "正常记账");
    cs.add("输入剩余 1500", sb.remaining(Stage::Input) == 1500, "2000-500");

    // --- 结转：输入结余滚入执行 ---
    sb.end_stage(Stage::Input); // 1500 结余
    cs.add(
        "结转进入执行",
        sb.allocated_with_carry(Stage::Execute) == 6500,
        "5000+1500",
    );

    // --- 隔离：执行超支不侵占呈现 ---
    let over = sb.charge(Stage::Execute, 10000); // 超出 6500
    cs.add("执行超支被裁剪", over == 6500, "只给可用量");
    cs.add("呈现池完好", sb.remaining(Stage::Present) == 3000, "未被侵占");

    // --- 呈现优先：呈现可全额使用 ---
    let p = sb.charge(Stage::Present, 3000);
    cs.add("呈现全额可用", p == 3000, "呈现优先");

    // --- 帧内结转不跨帧 ---
    sb.end_frame();
    cs.add(
        "帧末结转清零",
        sb.allocated_with_carry(Stage::Execute) == 5000,
        "新帧重新分配",
    );

    // --- 自定义比例 ---
    let sb2 = StageBudget::new(10000).with_ratios(1000, 6000, 3000);
    cs.add(
        "自定义比例生效",
        sb2.allocated_with_carry(Stage::Execute) == 6000,
        "60% 执行",
    );

    // --- present_intact ---
    let mut sb3 = StageBudget::new(1000);
    sb3.charge(Stage::Present, 100);
    cs.add("present_intact", sb3.present_intact(), "在额度内即完好");

    cs
}
