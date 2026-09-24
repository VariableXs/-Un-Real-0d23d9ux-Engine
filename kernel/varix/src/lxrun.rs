//! vxrun 与验收载体（WP-301 · B-402 LTP 合规子集 + B-406 三类载体全绿）。
//!
//! MD2 篇 4.5/4.6：vxrun 四件事——挂载视图/环境注入/ABI 登记/移交内核
//! 装载器，顺序不可跳；封闭依赖树磁盘占用上限 800MB（Electron 级），
//! 超限星卡登记（配额治理 19.3 的直插级条款）。柜台测试三层中的语义层：
//! **LTP 合规子集离线跑，失败用例逐个归因——柜台缺陷修柜台，语义取舍
//! 登记差异表**（失败项全部归因是 B-402 达标线）；应用层：三类验收载体
//! （二十个静态 CLI、VSCode、Java 与 Python 运行时）真实运行行为对照。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// vxrun 四步（篇 4.5）
// ---------------------------------------------------------------------------

/// vxrun 四步（顺序不可跳——挂载视图是环境注入的前提，ABI 登记是移交
/// 装载器的前提）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VxrunStep {
    /// 第一步：构造挂载视图（/compat/linux/<应用名>/ 根树挂为可见根）。
    MountView,
    /// 第二步：环境注入（PATH/HOME/LANG/代理/主题——按会话注入表）。
    EnvInject,
    /// 第三步：ABI 登记（解释器指到封闭树内 ld-linux，向柜台注册身份）。
    AbiRegister,
    /// 第四步：移交内核装载器（ELF 路）。
    HandoffLoader,
}

pub const VXRUN_STEPS: usize = 4;

pub const ALL_STEPS: [VxrunStep; VXRUN_STEPS] = [
    VxrunStep::MountView,
    VxrunStep::EnvInject,
    VxrunStep::AbiRegister,
    VxrunStep::HandoffLoader,
];

/// 步序校验：四步恰按规范序执行一次（跳步拒——与 B-2103 安装四段同族）。
pub fn steps_in_order(seq: &[VxrunStep; VXRUN_STEPS]) -> bool {
    let mut i = 0;
    while i < VXRUN_STEPS {
        if seq[i] != ALL_STEPS[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 封闭依赖树磁盘上限（篇 4.5：800MB，Electron 级）。
pub const CLOSED_TREE_MB_MAX: u64 = 800;

/// 封闭树预算裁决：超限返回 false（星卡登记——配额治理直插级条款）。
pub fn tree_budget_ok(tree_mb: u64) -> bool {
    tree_mb <= CLOSED_TREE_MB_MAX
}

// ---------------------------------------------------------------------------
// LTP 合规子集跑批与归因（B-402）
// ---------------------------------------------------------------------------

/// LTP 失败归因两分（柜台缺陷修柜台 / 语义取舍登记差异表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LtpAttrib {
    /// 柜台缺陷：翻译面 bug——修柜台（清零才算过闸）。
    CounterDefect,
    /// 语义取舍：设计面主动裁剪——登记差异表（B-404 的 LTP 侧入口）。
    SemanticTradeoff,
}

/// 一条 LTP 合规子集用例的跑批记录。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LtpCase {
    pub id: u32,
    pub passed: bool,
    /// 失败必归因：passed=false 时 attrib 必须是 Some（无未归因失败）。
    pub attrib: Option<LtpAttrib>,
}

/// 跑批报告三账：未归因失败 / 开口缺陷 / 语义取舍。
pub struct BatchReport {
    pub cases: usize,
    pub passed: usize,
    pub unattributed: usize,
    pub open_defects: usize,
    pub tradeoffs: usize,
}

/// 跑批对账（B-402 达标线的可计算形态）：失败项全部归因 + 柜台缺陷
/// 清零 + 取舍全数进差异表。
pub fn batch_report(cases: &[LtpCase]) -> BatchReport {
    let mut rep = BatchReport { cases: cases.len(), passed: 0, unattributed: 0, open_defects: 0, tradeoffs: 0 };
    let mut i = 0;
    while i < cases.len() {
        if cases[i].passed {
            rep.passed += 1;
        } else {
            match cases[i].attrib {
                Some(LtpAttrib::CounterDefect) => rep.open_defects += 1,
                Some(LtpAttrib::SemanticTradeoff) => rep.tradeoffs += 1,
                None => rep.unattributed += 1,
            }
        }
        i += 1;
    }
    rep
}

/// 过闸判据：未归因 = 0 且开口缺陷 = 0（取舍 > 0 合法——登记了就诚实）。
pub fn batch_gate(rep: &BatchReport) -> bool {
    rep.unattributed == 0 && rep.open_defects == 0
}

// ---------------------------------------------------------------------------
// 三类验收载体（B-406）
// ---------------------------------------------------------------------------

/// 验收载体三类（MD1 24.6：真实运行行为与 Linux 参考机对照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Carrier {
    /// 二十个静态 CLI。
    StaticCli,
    /// VSCode（Electron 专项，S304 标志性判据）。
    ElectronApp,
    /// Java 与 Python 运行时。
    RuntimeVm,
}

pub const CARRIER_ROWS: usize = 3;

pub const ALL_CARRIERS: [Carrier; CARRIER_ROWS] = [
    Carrier::StaticCli,
    Carrier::ElectronApp,
    Carrier::RuntimeVm,
];

/// 单类载体验收记录：启动四步齐 + 行为对照绿。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CarrierRun {
    pub class: Carrier,
    pub steps_ok: u8,
    pub green: bool,
}

/// 载体判绿：四步齐 + 行为对照绿（对照基准 = Linux 参考机模型面）。
pub fn carrier_green(r: &CarrierRun) -> bool {
    r.steps_ok == VXRUN_STEPS as u8 && r.green
}

// ---------------------------------------------------------------------------
// CheckSet（B-402/406 · 7 项）
// ---------------------------------------------------------------------------

pub fn run_lxrun_checks() -> CheckSet {
    let mut set = CheckSet::new("B-402/406 vxrun 与验收载体");
    // 1. vxrun 四步序：规范序通过、乱序拒。
    let good = ALL_STEPS;
    let bad = [VxrunStep::MountView, VxrunStep::EnvInject, VxrunStep::HandoffLoader, VxrunStep::AbiRegister];
    set.add(
        "B-402 vxrun 四步序",
        steps_in_order(&good) && !steps_in_order(&bad),
        "挂载视图/环境注入/ABI 登记/移交装载器——顺序不可跳（篇 4.5）",
    );
    // 2. LTP 子集跑批：用例账目一致。
    let mut cases = [
        LtpCase { id: 1, passed: true, attrib: None },
        LtpCase { id: 2, passed: false, attrib: Some(LtpAttrib::SemanticTradeoff) },
        LtpCase { id: 3, passed: true, attrib: None },
        LtpCase { id: 4, passed: false, attrib: None }, // 未归因——gate 必红
    ];
    let rep1 = batch_report(&cases);
    set.add(
        "B-402 LTP 子集跑批",
        rep1.cases == 4 && rep1.passed == 2 && rep1.unattributed == 1,
        "合规子集离线跑批模型——账目三分：归因/开口缺陷/取舍",
    );
    // 3. 失败全归因：未归因失败清零后过闸（柜台缺陷修柜台，取舍登记）。
    cases[3].passed = true; // 柜台缺陷修复后重跑通过
    let rep2 = batch_report(&cases);
    set.add(
        "B-402 失败全归因",
        batch_gate(&rep2) && rep2.tradeoffs == 1,
        "失败项全部归因——缺陷清零+取舍登记差异表（B-402 达标线）",
    );
    // 4. 归因闭环：缺陷修完+取舍登记后账目守恒（通过+取舍=全部——
    // 取舍用例合法地不过，不许拿"全绿"绑架"全归因"）。
    set.add(
        "B-402 归因闭环",
        rep2.unattributed == 0 && rep2.open_defects == 0 && rep2.passed + rep2.tradeoffs == rep2.cases,
        "修柜台+登差异表→失败项全部归因——离线跑通过（B-402 达标线）",
    );
    // 5. 三类载体登记。
    set.add(
        "B-406 三类载体登记",
        ALL_CARRIERS.len() == 3 && ALL_CARRIERS[0] == Carrier::StaticCli && ALL_CARRIERS[2] == Carrier::RuntimeVm,
        "静态 CLI×20 / VSCode Electron / Java 与 Python 运行时（MD1 24.6）",
    );
    // 6. 载体绿判：四步齐+行为对照绿，三类全绿。
    let runs = [
        CarrierRun { class: Carrier::StaticCli, steps_ok: 4, green: true },
        CarrierRun { class: Carrier::ElectronApp, steps_ok: 4, green: true },
        CarrierRun { class: Carrier::RuntimeVm, steps_ok: 4, green: true },
    ];
    let mut i6 = 0;
    let mut all_green = true;
    while i6 < CARRIER_ROWS {
        if !carrier_green(&runs[i6]) {
            all_green = false;
        }
        i6 += 1;
    }
    set.add(
        "B-406 载体绿判",
        all_green && !carrier_green(&CarrierRun { class: Carrier::StaticCli, steps_ok: 3, green: true }),
        "三类载体全绿=启动四步齐+行为对照绿——少一步即不判绿（对照 Linux 参考机）",
    );
    // 7. 封闭树上限：800MB 超限星卡登记。
    set.add(
        "B-406 封闭树上限",
        tree_budget_ok(800) && !tree_budget_ok(801),
        "封闭依赖树 800MB（Electron 级）——超限星卡登记（配额治理直插级条款）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单测（fd04 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fd04_vxrun_step_order() {
        assert!(steps_in_order(&ALL_STEPS));
        let bad = [VxrunStep::EnvInject, VxrunStep::MountView, VxrunStep::AbiRegister, VxrunStep::HandoffLoader];
        assert!(!steps_in_order(&bad));
        // 封闭树预算边界：恰 800 过，801 拒。
        assert!(tree_budget_ok(0));
        assert!(tree_budget_ok(CLOSED_TREE_MB_MAX));
        assert!(!tree_budget_ok(CLOSED_TREE_MB_MAX + 1));
    }

    #[test]
    fn fd04_ltp_attribution_closed_loop() {
        // 修复前：一个未归因 + 一个开口缺陷 → gate 必红。
        let before = [
            LtpCase { id: 1, passed: true, attrib: None },
            LtpCase { id: 2, passed: false, attrib: None },
            LtpCase { id: 3, passed: false, attrib: Some(LtpAttrib::CounterDefect) },
        ];
        let rep_before = batch_report(&before);
        assert!(!batch_gate(&rep_before));
        assert_eq!(rep_before.unattributed, 1);
        assert_eq!(rep_before.open_defects, 1);
        // 修复后：缺陷变通过、未归因归为取舍 → gate 绿。
        let after = [
            LtpCase { id: 1, passed: true, attrib: None },
            LtpCase { id: 2, passed: false, attrib: Some(LtpAttrib::SemanticTradeoff) },
            LtpCase { id: 3, passed: true, attrib: None },
        ];
        let rep_after = batch_report(&after);
        assert!(batch_gate(&rep_after));
        assert_eq!(rep_after.tradeoffs, 1);
    }

    #[test]
    fn fd04_carriers_three_green() {
        for c in ALL_CARRIERS {
            assert!(carrier_green(&CarrierRun { class: c, steps_ok: 4, green: true }));
            assert!(!carrier_green(&CarrierRun { class: c, steps_ok: 4, green: false }));
            assert!(!carrier_green(&CarrierRun { class: c, steps_ok: 2, green: true }));
        }
    }

    #[test]
    fn fd04_batch_tally_identity() {
        // 账目守恒：cases == passed + 未归因 + 开口缺陷 + 取舍。
        let cases = [
            LtpCase { id: 1, passed: true, attrib: None },
            LtpCase { id: 2, passed: true, attrib: None },
            LtpCase { id: 3, passed: false, attrib: Some(LtpAttrib::CounterDefect) },
            LtpCase { id: 4, passed: false, attrib: Some(LtpAttrib::SemanticTradeoff) },
            LtpCase { id: 5, passed: false, attrib: None },
        ];
        let rep = batch_report(&cases);
        assert_eq!(rep.cases, rep.passed + rep.unattributed + rep.open_defects + rep.tradeoffs);
        assert_eq!(rep.passed, 2);
        assert_eq!(rep.unattributed, 1);
    }
}
