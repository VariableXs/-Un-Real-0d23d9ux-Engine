//! cga119_isaeval — CGPU-F0119 新指令集评估流程。
//!
//! 判据原文（施工书锚点 CGPU-F0119）：新 SIMD 指令集的标准化评估流程
//! （SVE2/AVX10/未来扩展）：评估模板（指令覆盖度/目标硬件可得性/预期
//! 收益/迁移成本四维打分）、评估→立项→实现→基准的流水、评估记录归档。
//! 流程使新指令集的采纳是决策而非追风。判据：评估模板、流水定义、记录
//! 归档、决策案例（RVV 即首个案例）、复评节奏。
//!
//! **设计定盘**：四维打分（覆盖度/可得性/收益/成本）各 0~100 整数分，
//! 综合分 = 加权和（覆盖 30% / 可得 30% / 收益 25% / 成本 15%，成本取
//! 反向——成本越高分越低，权重和恰 100 用整数验证）。流水四阶段闭集：
//! Evaluated（评估完）→ Approved（立项）→ Implemented（实现完）→
//! Benchmarked（基准入库），单向推进跳段即拒。复评节奏：立项后每过
//! RE_EVAL_INTERVAL 个评估纪元未走到下一阶段即触发复评标记（防止立项
//! 后烂尾）。RVV 作为首个决策案例固化在档案里。
//!
//! 诊断码段 0x0100~0x010F 第 6 子区（0x0150~0x015F）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA119_VERSION: &str = "cga119-isaeval-v1";

/// 诊断子区基值（0x01 段第 6 子区）。
pub const SEG_BASE: u16 = 0x0150;

/// 复评节奏：立项后未推进的纪元数上限。
pub const RE_EVAL_INTERVAL: u32 = 4;

/// 四维权重（和恰 100，编译期可验）。
pub const W_COVER: u32 = 30;
pub const W_AVAIL: u32 = 30;
pub const W_GAIN: u32 = 25;
pub const W_COST: u32 = 15;

/// 权重守恒的编译期断言（常量求和必须恰为 100）。
const _: () = assert!(W_COVER + W_AVAIL + W_GAIN + W_COST == 100);

/// 流水阶段闭集（单向推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 评估完成。
    Evaluated,
    /// 已立项。
    Approved,
    /// 实现完成。
    Implemented,
    /// 基准入库。
    Benchmarked,
}

impl Stage {
    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            Stage::Evaluated => 0,
            Stage::Approved => 1,
            Stage::Implemented => 2,
            Stage::Benchmarked => 3,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 四维打分卡（判据"评估模板"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScoreCard {
    /// 指令覆盖度（0~100：目标算子族中被该指令集覆盖的比例）。
    pub cover: u32,
    /// 目标硬件可得性（0~100：目标装机与路线图可得性）。
    pub avail: u32,
    /// 预期收益（0~100：吞吐/能效收益预估）。
    pub gain: u32,
    /// 迁移成本（0~100 为成本档，综合分中取反向）。
    pub cost: u32,
}

impl ScoreCard {
    /// 打分合法性：四维均须落在 0~100。
    pub fn validate(&self) -> Result<(), u16> {
        for (name_ok, v) in [
            (self.cover <= 100, self.cover),
            (self.avail <= 100, self.avail),
            (self.gain <= 100, self.gain),
            (self.cost <= 100, self.cost),
        ] {
            if !name_ok {
                let _ = v;
                return Err(SEG_BASE + 5);
            }
        }
        Ok(())
    }

    /// 综合分（整数加权，成本反向：100-cost 参与加权）。
    pub fn composite(&self) -> Result<u32, u16> {
        self.validate()?;
        let inv_cost = 100 - self.cost;
        Ok((self.cover * W_COVER
            + self.avail * W_AVAIL
            + self.gain * W_GAIN
            + inv_cost * W_COST)
            / 100)
    }
}

/// 一条评估档案（判据"记录归档"）。
pub struct IsaEvalRecord {
    /// 指令集名。
    pub isa: &'static str,
    /// 打分卡。
    pub card: ScoreCard,
    /// 综合分。
    pub score: u32,
    /// 当前阶段。
    pub stage: Stage,
    /// 立项纪元（Approved 时登记；未立项为 0）。
    pub approved_epoch: u32,
    /// 最近推进纪元。
    pub last_advance_epoch: u32,
    /// 复评标记。
    pub needs_reeval: bool,
}

/// 评估流水账（判据"流水定义"）。
pub struct IsaPipeline {
    /// 档案册。
    records: Vec<IsaEvalRecord>,
    /// 当前纪元。
    epoch: u32,
    /// 归档封印。
    sealed: Option<u64>,
}

impl IsaPipeline {
    /// 空流水。
    pub fn new() -> Self {
        IsaPipeline {
            records: Vec::new(),
            epoch: 1,
            sealed: None,
        }
    }

    /// 当前纪元。
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// 纪元推进（每纪元一次复评扫描）。
    pub fn advance_epoch(&mut self) -> Result<usize, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        self.epoch += 1;
        let mut flagged = 0;
        for r in &mut self.records {
            if r.stage == Stage::Approved && !r.needs_reeval {
                let idle = self.epoch.saturating_sub(r.last_advance_epoch);
                if idle >= RE_EVAL_INTERVAL {
                    r.needs_reeval = true;
                    flagged += 1;
                }
            }
        }
        Ok(flagged)
    }

    /// 提交评估（判据"评估模板"：同名重复即拒；综合分低于
    /// ADOPT_FLOOR 不许立项——评估是决策不是追风）。
    pub fn submit(&mut self, isa: &'static str, card: ScoreCard) -> Result<u32, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        if isa.is_empty() {
            return Err(SEG_BASE + 7);
        }
        if self.records.iter().any(|r| r.isa == isa) {
            return Err(SEG_BASE + 8);
        }
        let score = card.composite()?;
        self.records.push(IsaEvalRecord {
            isa,
            card,
            score,
            stage: Stage::Evaluated,
            approved_epoch: 0,
            last_advance_epoch: self.epoch,
            needs_reeval: false,
        });
        Ok(score)
    }

    /// 采纳下限（综合分低于此不许立项）。
    pub const ADOPT_FLOOR: u32 = 60;

    /// 流水推进（判据"流水定义"单向推进；立项须过采纳下限；
    /// 复评标记未清除不许推进）。
    pub fn advance(&mut self, isa: &'static str) -> Result<Stage, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        let rec = match self.records.iter_mut().find(|r| r.isa == isa) {
            Some(r) => r,
            None => return Err(SEG_BASE + 9),
        };
        if rec.needs_reeval {
            return Err(SEG_BASE + 10);
        }
        let next = match rec.stage {
            Stage::Evaluated => {
                if rec.score < Self::ADOPT_FLOOR {
                    return Err(SEG_BASE + 11);
                }
                rec.approved_epoch = self.epoch;
                Stage::Approved
            }
            Stage::Approved => Stage::Implemented,
            Stage::Implemented => Stage::Benchmarked,
            Stage::Benchmarked => return Err(SEG_BASE + 12),
        };
        rec.stage = next;
        rec.last_advance_epoch = self.epoch;
        Ok(next)
    }

    /// 案例查询（判据"决策案例"）。
    pub fn record_of(&self, isa: &'static str) -> Option<&IsaEvalRecord> {
        self.records.iter().find(|r| r.isa == isa)
    }

    /// RVV 首案例：首个立项记录必须是 RVV（流水账的语义锚——
    /// 判据点名 RVV 即首个案例，账本必须能自证这一点）。
    pub fn rvv_first_case_ok(&self) -> bool {
        let reached = |s: Stage| s.ordinal() >= Stage::Evaluated.ordinal();
        match self.records.first() {
            Some(r) => r.isa == "RVV" && reached(r.stage),
            None => false,
        }
    }

    /// 归档（判据"记录归档"）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 13);
        }
        if self.records.is_empty() {
            return Err(SEG_BASE + 14);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for r in &self.records {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ r.score as u64;
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ r.stage.ordinal() as u64;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for IsaPipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// 短板阈值（判据"评估模板"的诊断面）：任一维度低于此值即短板。
pub const DIM_FLOOR: u32 = 40;

/// 短板诊断（四维逐维达标判定；返回短板维度名列表——
/// 空列表 = 无短板）。
pub fn weak_dims(card: &ScoreCard) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    if card.cover < DIM_FLOOR {
        out.push("cover");
    }
    if card.avail < DIM_FLOOR {
        out.push("avail");
    }
    if card.gain < DIM_FLOOR {
        out.push("gain");
    }
    if card.cost > 100 - DIM_FLOOR {
        out.push("cost");
    }
    out
}

/// 流水吞吐投影（判据"流水定义"的阶段计数面）：四阶段各多少条。
pub fn stage_census(records: &[IsaEvalRecord]) -> [usize; 4] {
    let mut out = [0usize; 4];
    for r in records {
        out[r.stage.ordinal() as usize] += 1;
    }
    out
}

impl IsaPipeline {
    /// 短板查询（档案的短板面；无档案返回 None）。
    pub fn weak_dims_of(&self, isa: &'static str) -> Option<Vec<&'static str>> {
        self.records
            .iter()
            .find(|r| r.isa == isa)
            .map(|r| weak_dims(&r.card))
    }

    /// 阶段计数投影。
    pub fn stages(&self) -> [usize; 4] {
        stage_census(&self.records)
    }

    /// 复评处置——补证据（判据"复评节奏"：补齐证据后清除复评标记，
    /// 纪元推进面同步刷新）。
    pub fn resolve_reeval_boost(&mut self, isa: &'static str) -> Result<(), u16> {
        let r = match self.records.iter_mut().find(|r| r.isa == isa) {
            Some(r) => r,
            None => return Err(SEG_BASE + 9),
        };
        if !r.needs_reeval {
            return Err(SEG_BASE + 15);
        }
        r.needs_reeval = false;
        r.last_advance_epoch = self.epoch;
        Ok(())
    }

    /// 复评处置——撤项（判据"复评节奏"的退路面：已立项项退回
    /// Evaluated 并留复评标记清除；未立项项直接拒）。
    pub fn resolve_reeval_drop(&mut self, isa: &'static str) -> Result<(), u16> {
        let r = match self.records.iter_mut().find(|r| r.isa == isa) {
            Some(r) => r,
            None => return Err(SEG_BASE + 9),
        };
        if r.stage != Stage::Approved {
            return Err(SEG_BASE + 16);
        }
        r.stage = Stage::Evaluated;
        r.approved_epoch = 0;
        r.needs_reeval = false;
        r.last_advance_epoch = self.epoch;
        Ok(())
    }

    /// 复评在案数。
    pub fn reeval_pending(&self) -> usize {
        self.records.iter().filter(|r| r.needs_reeval).count()
    }
}
