//! 运行时画像模板与入库制度（WP-304 · B-3303 五步体检单成文入库）。
//!
//! MD2 篇 33.3：Java 与 Python 的画像立例之后，"新运行时如何直插"有了
//! 模板——**内存换算、线程依赖、时钟精度、文件语义、双载体判例五步走**，
//! Go/Rust 静态类（免费直通）之外的每一类动态运行时照单体检。画像结论进
//! 星卡（运行时级注记）与差异表（篇 4 补充清单）——**柜台层的运行时知识
//! 沉淀为资产而非个人经验**。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 五步体检单（穷举——"单"的意思是缺一步就不是体检是玄学）
// ---------------------------------------------------------------------------

/// 体检步数。
pub const CHECKUP_STEPS: usize = 5;

/// 五步体检（穷举——MD2 33.3 明文五步，步序即体检序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckupStep {
    /// 内存换算（如 JVM 堆七成设参）。
    MemoryConversion,
    /// 线程依赖（clone/futex 形态）。
    ThreadDeps,
    /// 时钟精度（clock_gettime 精度与单调性）。
    ClockPrecision,
    /// 文件语义（锁文件/临时目录在 ext4）。
    FsSemantics,
    /// 双载体判例（服务端与图形端/脚本与服务各一条路）。
    DualCarrier,
}

/// 体检单步结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StepVerdict {
    pub step: CheckupStep,
    /// 该步体检结论（绿/有差异——差异进差异表，不是失败）。
    pub green: bool,
    /// 已登记（没登记的步骤是没体检——"没遇到过"不是"没登记"的理由）。
    pub recorded: bool,
}

/// 画像模板（一类动态运行时一张单——照单体检）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProfileTemplate {
    /// 五步全有结论（漏一步的单是半张单）。
    pub steps: [Option<StepVerdict>; CHECKUP_STEPS],
    /// 模板格式版本。
    pub version: u32,
}

/// 模板入库版本（首版钉死——版本漂移走 ADR）。
pub const TEMPLATE_VER: u32 = 1;

impl ProfileTemplate {
    /// 空单（五步全空——体检从零开始，不许默认绿）。
    pub const fn blank() -> ProfileTemplate {
        ProfileTemplate { steps: [None; CHECKUP_STEPS], version: TEMPLATE_VER }
    }

    /// 模板完整：五步全登记且格式版本钉死（**B-3303 达标线的第一半**）。
    pub fn complete(&self) -> bool {
        if self.version != TEMPLATE_VER {
            return false;
        }
        let mut i = 0;
        while i < CHECKUP_STEPS {
            if self.steps[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 步骤到下标的穷举映射（单内步序 = 五步定义序——乱序登记不算体检完）。
pub fn step_slot(s: CheckupStep) -> usize {
    match s {
        CheckupStep::MemoryConversion => 0,
        CheckupStep::ThreadDeps => 1,
        CheckupStep::ClockPrecision => 2,
        CheckupStep::FsSemantics => 3,
        CheckupStep::DualCarrier => 4,
    }
}

/// 登记一步结论（下标由 step_slot 单源决定——重复登记覆盖不追认）。
pub fn record_step(t: &mut ProfileTemplate, v: StepVerdict) {
    t.steps[step_slot(v.step)] = Some(v);
}

// ---------------------------------------------------------------------------
// 成文入库与结论双消费者
// ---------------------------------------------------------------------------

/// 模板台账条目（**成文入库 = 模板完整且 filed**——半张单不入库，
/// 入库的完整单是后续运行时的体检模板）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FiledTemplate {
    pub template: ProfileTemplate,
    pub filed: bool,
}

impl FiledTemplate {
    /// 入库有效判（**B-3303 达标线**）。
    pub fn valid(&self) -> bool {
        self.filed && self.template.complete()
    }
}

/// 结论双消费者挂号（星卡运行时级注记 + 差异表补充清单——**两处都挂才算
/// 沉淀为资产**，只进一处是私有笔记不是制度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConclusionDispersion {
    pub to_starcard_note: bool,
    pub to_diff_table: bool,
}

impl ConclusionDispersion {
    pub fn settled(&self) -> bool {
        self.to_starcard_note && self.to_diff_table
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-3303 · 3 项）
// ---------------------------------------------------------------------------

pub fn run_rtmpl_checks() -> CheckSet {
    let mut set = CheckSet::new("B-3303 运行时画像模板");
    // 1. 五步穷举：步序映射穷举且与定义序一致（乱序登记不算体检完）。
    let slots = [
        step_slot(CheckupStep::MemoryConversion),
        step_slot(CheckupStep::ThreadDeps),
        step_slot(CheckupStep::ClockPrecision),
        step_slot(CheckupStep::FsSemantics),
        step_slot(CheckupStep::DualCarrier),
    ];
    let mut all_slots = true;
    let mut i = 0;
    while i < slots.len() {
        if slots[i] != i {
            all_slots = false;
        }
        i += 1;
    }
    set.add(
        "B-3303 五步穷举",
        all_slots && CHECKUP_STEPS == 5,
        "内存换算/线程依赖/时钟精度/文件语义/双载体判例——五步照单走，玄学变 checklist",
    );
    // 2. 成文入库（B-3303 达标线）：完整单才可入库，半张单拒收。
    let mut t = ProfileTemplate::blank();
    let steps = [
        CheckupStep::MemoryConversion,
        CheckupStep::ThreadDeps,
        CheckupStep::ClockPrecision,
        CheckupStep::FsSemantics,
        CheckupStep::DualCarrier,
    ];
    i = 0;
    while i < steps.len() {
        record_step(&mut t, StepVerdict { step: steps[i], green: true, recorded: true });
        i += 1;
    }
    let full = FiledTemplate { template: t, filed: true };
    let mut half = ProfileTemplate::blank();
    record_step(&mut half, StepVerdict { step: CheckupStep::MemoryConversion, green: true, recorded: true });
    let half_filed = FiledTemplate { template: half, filed: true };
    let unfiled = FiledTemplate { template: t, filed: false };
    set.add(
        "B-3303 体检单成文入库",
        full.valid() && !half_filed.valid() && !unfiled.valid(),
        "五步全登记且 filed 才算入库——半张单与抽屉里的完整单都不算（B-3303 达标线）",
    );
    // 3. 结论双消费者：星卡注记 + 差异表两处挂号（沉淀为资产非个人经验）。
    let settled = ConclusionDispersion { to_starcard_note: true, to_diff_table: true };
    let starcard_only = ConclusionDispersion { to_starcard_note: true, to_diff_table: false };
    set.add(
        "B-3303 结论双消费者",
        settled.settled() && !starcard_only.settled(),
        "画像结论进星卡注记与差异表补充清单——只进一处是私有笔记不是制度",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe13 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe13_step_slot_order() {
        // 五步映射恒等：定义序即存储序。
        assert_eq!(step_slot(CheckupStep::MemoryConversion), 0);
        assert_eq!(step_slot(CheckupStep::ThreadDeps), 1);
        assert_eq!(step_slot(CheckupStep::ClockPrecision), 2);
        assert_eq!(step_slot(CheckupStep::FsSemantics), 3);
        assert_eq!(step_slot(CheckupStep::DualCarrier), 4);
    }

    #[test]
    fn fe13_blank_template_incomplete() {
        // 空单不完整——体检从零开始，不许默认绿。
        let mut t = ProfileTemplate::blank();
        assert!(!t.complete());
        record_step(&mut t, StepVerdict { step: CheckupStep::DualCarrier, green: true, recorded: true });
        assert!(!t.complete()); // 一/五 ≠ 全
        assert!(t.steps[step_slot(CheckupStep::DualCarrier)].is_some());
    }

    #[test]
    fn fe13_record_overwrites_by_slot() {
        // 同步重复登记按槽覆盖：单源下标，不产生影子条目。
        let mut t = ProfileTemplate::blank();
        record_step(&mut t, StepVerdict { step: CheckupStep::ClockPrecision, green: false, recorded: true });
        record_step(&mut t, StepVerdict { step: CheckupStep::ClockPrecision, green: true, recorded: true });
        let slot = step_slot(CheckupStep::ClockPrecision);
        assert_eq!(t.steps[slot].unwrap().green, true);
        // 其余槽仍空。
        assert!(t.steps[step_slot(CheckupStep::MemoryConversion)].is_none());
    }

    #[test]
    fn fe13_filing_rules() {
        let mut t = ProfileTemplate::blank();
        let steps = [
            CheckupStep::MemoryConversion,
            CheckupStep::ThreadDeps,
            CheckupStep::ClockPrecision,
            CheckupStep::FsSemantics,
            CheckupStep::DualCarrier,
        ];
        let mut i = 0;
        while i < steps.len() {
            record_step(&mut t, StepVerdict { step: steps[i], green: i != 3, recorded: true });
            i += 1;
        }
        // 差异步（文件语义红）不影响入库资格——差异进差异表不是失败。
        assert!(t.complete());
        assert!(FiledTemplate { template: t, filed: true }.valid());
        // 版本漂移的单不完整。
        let mut drift = ProfileTemplate::blank();
        drift.version = 99;
        let mut i = 0;
        while i < steps.len() {
            record_step(&mut drift, StepVerdict { step: steps[i], green: true, recorded: true });
            i += 1;
        }
        assert!(!drift.complete());
    }
}
