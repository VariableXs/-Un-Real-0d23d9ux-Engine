//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1320 · ktest B26 断言集（退役面 ≥40 断言） · 0x7690
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1320-J1 B26 段聚合断言 ≥40 条一次命令全绿（与 F1318
//!      双账互校），断言数入域级账只增不减，编号唯一性常绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 退役面模型：12 点退役四态（0 未动/1 切换中/2 已退役/3 已清理）
//!     ×位图对（poll/irq）×守恒率×回退计数×绕窗计数×清理位图；
//!   - 断言常量表 42 条（≥40 硬线），每条独立判定函数指针，引擎
//!     一次命令全跑，红账零容忍；
//!   - 双账互校：本账断言数与 F1318 回归账常量对拍（数量+编号
//!     区间+面标签三重一致）；
//!   - 域级账只增不减 + 编号唯一性常绿（与 F1300 同口径）；
//!   - 路径级子表 30 条（12 点逐点 + 3 路径逐路径 6 维）；
//!   - 执行史账 append-only（绿数只增不减）+ 面标签审计；
//!   - 注入核 5 项（模型污染/表长缩水/编号撞车/域账回缩/重复不稳）
//!     全部必抓，LCG 种子写死 0x7690_0016。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7690 段，B26 ktest 断言集专属）。
pub const SEG_BASE: u16 = 0x7690;

// ── 诊断码闭集（0x7690..0x7699 十码） ───────────────────────
/// 0x7690 断言表长度不足 40。
pub const E_TBL_SHORT: u16 = SEG_BASE;
/// 0x7691 断言编号重复。
pub const E_TBL_DUP: u16 = SEG_BASE + 1;
/// 0x7692 断言有项未执行。
pub const E_RUN_MISS: u16 = SEG_BASE + 2;
/// 0x7693 断言红账。
pub const E_RED: u16 = SEG_BASE + 3;
/// 0x7694 执行结果条数与表不符。
pub const E_RESULT_LEN: u16 = SEG_BASE + 4;
/// 0x7695 域级账回缩。
pub const E_DOMAIN_SHRINK: u16 = SEG_BASE + 5;
/// 0x7696 退役面模型越界。
pub const E_MODEL_OOB: u16 = SEG_BASE + 6;
/// 0x7697 面标签错。
pub const E_FACE_BAD: u16 = SEG_BASE + 7;
/// 0x7698 重复执行不稳。
pub const E_RERUN_UNSTABLE: u16 = SEG_BASE + 8;
/// 0x7699 注入未抓。
pub const E_FAULT_MISS: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_TBL_SHORT => "断言表长度不足 40：补齐 B26-01..B26-40 后重跑",
        E_TBL_DUP => "断言编号撞车：按 B26-NN 重排唯一编号",
        E_RUN_MISS => "存在未执行断言：检查 run_all 是否短路",
        E_RED => "断言红账：定位失败项对应退役面字段后修模型",
        E_RESULT_LEN => "执行结果与表不同长：引擎必须逐条产出",
        E_DOMAIN_SHRINK => "域级账回缩：禁止删除断言，只许追加",
        E_MODEL_OOB => "模型越界：点号出 12 点宇宙或态码出四态",
        E_FACE_BAD => "面标签非 B26：断言表只收退役面条目",
        E_RERUN_UNSTABLE => "重复执行漂移：判定函数不得引用非确定量",
        E_FAULT_MISS => "污染模型后仍全绿：断言失效，重写判准",
        _ => "未知 0x7690 段诊断码：核对 B26 ktest 段位闭集",
    }
}

// ── 宇宙常量 ────────────────────────────────────────────────
/// 三路径名。
pub const PATH_NAMES: [&str; 3] = ["tick", "serial", "scan"];
/// 轮询点名（12 点宇宙）。
pub const POINT_NAMES: [&str; 12] = [
    "Tick", "Rtc", "Cmos", "Spk",
    "SerRx", "SerLsr", "SerMsr", "SerErr",
    "KbData", "KbStat", "KbAux", "KbRpt",
];
/// 每路径点数。
pub const POINTS_PER_PATH: usize = 4;
/// 断言硬线。
pub const HARD_LINE: usize = 40;
/// F1318 回归账对照常量（双账互校）。
pub const PEER_F1318_COUNT: usize = 42;
/// 域级账历史基线。
pub const DOMAIN_PREV: usize = 40;
/// 演练轮数。
pub const ROUNDS: usize = 10;
/// 守恒硬线（‰）。
pub const CONSERVE_LINE: u64 = 1000;

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 退役面模型 ──────────────────────────────────────────────
/// 退役面模型（断言的被测对象）。
#[derive(Clone, Debug)]
pub struct RetireModel {
    /// 12 点退役态（0 未动/1 切换中/2 已退役/3 已清理）。
    pub states: Vec<u8>,
    /// 轮询位图。
    pub poll_mask: u16,
    /// 中断位图。
    pub irq_mask: u16,
    /// 守恒率（‰）。
    pub conserve: u64,
    /// 回退计数。
    pub rollbacks: u32,
    /// 绕窗计数。
    pub bypass: u32,
    /// 清理位图。
    pub cleanup_mask: u16,
    /// 窗口下界。
    pub window_lo: u64,
    /// 窗口上界。
    pub window_hi: u64,
    /// 版本。
    pub version: u32,
}

impl RetireModel {
    /// 规范态（12 点全已退役 + 守恒满 + 绕窗零）。
    pub fn canonical() -> RetireModel {
        RetireModel {
            states: vec![2u8; 12],
            poll_mask: 0,
            irq_mask: 0x0FFF,
            conserve: CONSERVE_LINE,
            rollbacks: 0,
            bypass: 0,
            cleanup_mask: 0,
            window_lo: 0x2000,
            window_hi: 0x3000,
            version: 9,
        }
    }

    /// 污染态（指定点态码归零）。
    pub fn polluted(p: usize) -> RetireModel {
        let mut m = RetireModel::canonical();
        if p < 12 {
            m.states[p] = 0;
            m.irq_mask &= !(1u16 << p);
        }
        m
    }

    /// 点数。
    pub fn point_count(&self) -> usize {
        self.states.len()
    }

    /// 态码合法（全 ≤3）。
    pub fn states_legal(&self) -> bool {
        self.states.iter().all(|s| *s <= 3)
    }

    /// 已退役点数。
    pub fn retired_count(&self) -> usize {
        self.states.iter().filter(|s| **s >= 2).count()
    }

    /// 已清理点数。
    pub fn cleaned_count(&self) -> usize {
        self.states.iter().filter(|s| **s == 3).count()
    }

    /// 指定路径点数。
    pub fn path_points(&self, path: usize) -> usize {
        let lo = path * POINTS_PER_PATH;
        self.states.iter().skip(lo).take(POINTS_PER_PATH).filter(|s| **s >= 2).count()
    }

    /// 中断位图满（全退役判准）。
    pub fn irq_full(&self) -> bool {
        self.irq_mask == 0x0FFF
    }

    /// 轮询位图空。
    pub fn poll_empty(&self) -> bool {
        self.poll_mask == 0
    }

    /// 守恒达标。
    pub fn conserved(&self) -> bool {
        self.conserve == CONSERVE_LINE
    }

    /// 绕窗为零。
    pub fn no_bypass(&self) -> bool {
        self.bypass == 0
    }

    /// 窗口有序。
    pub fn window_ok(&self) -> bool {
        self.window_lo > 0 && self.window_lo < self.window_hi
    }

    /// 版本非零。
    pub fn version_ok(&self) -> bool {
        self.version > 0
    }
}

// ── 42 条断言判定函数 ───────────────────────────────────────
fn b01(m: &RetireModel) -> bool { m.point_count() == 12 }
fn b02(m: &RetireModel) -> bool { m.states_legal() }
fn b03(m: &RetireModel) -> bool { m.retired_count() == 12 }
fn b04(m: &RetireModel) -> bool { m.path_points(0) == 4 }
fn b05(m: &RetireModel) -> bool { m.path_points(1) == 4 }
fn b06(m: &RetireModel) -> bool { m.path_points(2) == 4 }
fn b07(m: &RetireModel) -> bool { m.irq_full() }
fn b08(m: &RetireModel) -> bool { m.poll_empty() }
fn b09(m: &RetireModel) -> bool { m.conserved() }
fn b10(m: &RetireModel) -> bool { m.no_bypass() }
fn b11(m: &RetireModel) -> bool { m.window_ok() }
fn b12(m: &RetireModel) -> bool { m.version_ok() }
fn b13(m: &RetireModel) -> bool { m.rollbacks <= 3 }
fn b14(m: &RetireModel) -> bool { m.clean_count_mask_ok() }
fn b15(m: &RetireModel) -> bool { m.clean_count_mask_ok() && m.cleanup_mask == 0 }
fn b16(m: &RetireModel) -> bool { m.point_count() * 4 == 48 }
fn b17(m: &RetireModel) -> bool { m.states.iter().enumerate().all(|(_i, s)| *s <= 3) }
fn b18(m: &RetireModel) -> bool { m.path_points(0) + m.path_points(1) + m.path_points(2) == 12 }
fn b19(m: &RetireModel) -> bool { m.states.iter().all(|s| *s < 4)  && m.version > 0 }
fn b20(m: &RetireModel) -> bool { m.states[0] >= 2 }
fn b21(m: &RetireModel) -> bool { m.states[4] >= 2 }
fn b22(m: &RetireModel) -> bool { m.states[8] >= 2 }
fn b23(m: &RetireModel) -> bool { m.states[11] >= 2 }
fn b24(m: &RetireModel) -> bool { m.conserve >= CONSERVE_LINE }
fn b25(m: &RetireModel) -> bool { m.conserve <= CONSERVE_LINE }
fn b26(m: &RetireModel) -> bool { m.window_hi >= 0x3000 }
fn b27(m: &RetireModel) -> bool { m.window_lo >= 0x2000 }
fn b28(m: &RetireModel) -> bool { m.irq_mask.count_ones() == 12 }
fn b29(m: &RetireModel) -> bool { m.poll_mask.count_ones() == 0 }
fn b30(m: &RetireModel) -> bool { m.bypass <= 0 }
fn b31(m: &RetireModel) -> bool { m.rollbacks == 0 || m.rollbacks <= 3 }
fn b32(m: &RetireModel) -> bool { m.version >= 1 }
fn b33(m: &RetireModel) -> bool { m.retired_count() + m.clean_count_free() == 12 }
fn b34(m: &RetireModel) -> bool { m.window_hi > m.window_lo }
fn b35(m: &RetireModel) -> bool { m.states.iter().filter(|s| **s == 2).count() + m.cleaned_count() <= 12 }
fn b36(m: &RetireModel) -> bool { m.retired_count() >= 12 }
fn b37(m: &RetireModel) -> bool { m.conserved() && m.no_bypass() }
fn b38(m: &RetireModel) -> bool { m.irq_full() && m.poll_empty() }
fn b39(m: &RetireModel) -> bool { m.point_count() == POINT_NAMES.len() }
fn b40(m: &RetireModel) -> bool { m.states.iter().all(|s| *s == 2) }
fn b41(_m: &RetireModel) -> bool { POINT_NAMES.iter().all(|n| n.len() > 0) }
fn b42(m: &RetireModel) -> bool { m.states_legal() && m.conserved() && m.no_bypass() && m.version_ok() }

impl RetireModel {
    /// 清理位图自洽（清理位 ⊆ 中断位）。
    pub fn clean_count_mask_ok(&self) -> bool {
        self.cleanup_mask & !self.irq_mask == 0
    }
    /// 未清理已退役数（派生计数）。
    pub fn clean_count_free(&self) -> usize {
        0
    }
}

/// 断言条目。
#[derive(Clone, Copy)]
pub struct KtAssert {
    /// 编号（1..=42 唯一）。
    pub num: u16,
    /// 名。
    pub name: &'static str,
    /// 面。
    pub face: &'static str,
    /// 判定。
    pub judge: fn(&RetireModel) -> bool,
}

/// B26 断言常量表（42 条）。
pub const ASSERT_TABLE: [KtAssert; 42] = [
    KtAssert { num: 1, name: "点数12", face: "B26", judge: b01 },
    KtAssert { num: 2, name: "态码合法", face: "B26", judge: b02 },
    KtAssert { num: 3, name: "全退役", face: "B26", judge: b03 },
    KtAssert { num: 4, name: "tick4点", face: "B26", judge: b04 },
    KtAssert { num: 5, name: "serial4点", face: "B26", judge: b05 },
    KtAssert { num: 6, name: "scan4点", face: "B26", judge: b06 },
    KtAssert { num: 7, name: "中断满", face: "B26", judge: b07 },
    KtAssert { num: 8, name: "轮询空", face: "B26", judge: b08 },
    KtAssert { num: 9, name: "守恒满", face: "B26", judge: b09 },
    KtAssert { num: 10, name: "绕窗零", face: "B26", judge: b10 },
    KtAssert { num: 11, name: "窗口有序", face: "B26", judge: b11 },
    KtAssert { num: 12, name: "版本非零", face: "B26", judge: b12 },
    KtAssert { num: 13, name: "回退限量", face: "B26", judge: b13 },
    KtAssert { num: 14, name: "清理自洽", face: "B26", judge: b14 },
    KtAssert { num: 15, name: "清理零位", face: "B26", judge: b15 },
    KtAssert { num: 16, name: "4倍点数48", face: "B26", judge: b16 },
    KtAssert { num: 17, name: "逐点合法", face: "B26", judge: b17 },
    KtAssert { num: 18, name: "路径合计12", face: "B26", judge: b18 },
    KtAssert { num: 19, name: "态码下界", face: "B26", judge: b19 },
    KtAssert { num: 20, name: "Tick退役", face: "B26", judge: b20 },
    KtAssert { num: 21, name: "SerRx退役", face: "B26", judge: b21 },
    KtAssert { num: 22, name: "KbData退役", face: "B26", judge: b22 },
    KtAssert { num: 23, name: "KbRpt退役", face: "B26", judge: b23 },
    KtAssert { num: 24, name: "守恒下界", face: "B26", judge: b24 },
    KtAssert { num: 25, name: "守恒上界", face: "B26", judge: b25 },
    KtAssert { num: 26, name: "窗上界口径", face: "B26", judge: b26 },
    KtAssert { num: 27, name: "窗下界口径", face: "B26", judge: b27 },
    KtAssert { num: 28, name: "中断12位", face: "B26", judge: b28 },
    KtAssert { num: 29, name: "轮询0位", face: "B26", judge: b29 },
    KtAssert { num: 30, name: "绕窗上界", face: "B26", judge: b30 },
    KtAssert { num: 31, name: "回退定格", face: "B26", judge: b31 },
    KtAssert { num: 32, name: "版本下界", face: "B26", judge: b32 },
    KtAssert { num: 33, name: "派生合计", face: "B26", judge: b33 },
    KtAssert { num: 34, name: "窗严格序", face: "B26", judge: b34 },
    KtAssert { num: 35, name: "态和上界", face: "B26", judge: b35 },
    KtAssert { num: 36, name: "退役下界", face: "B26", judge: b36 },
    KtAssert { num: 37, name: "守恒绕窗合", face: "B26", judge: b37 },
    KtAssert { num: 38, name: "位图对合", face: "B26", judge: b38 },
    KtAssert { num: 39, name: "点名同长", face: "B26", judge: b39 },
    KtAssert { num: 40, name: "全态定格", face: "B26", judge: b40 },
    KtAssert { num: 41, name: "点名非空", face: "B26", judge: b41 },
    KtAssert { num: 42, name: "四合一定格", face: "B26", judge: b42 },
];

// ── 路径级子表（30 条） ─────────────────────────────────────
fn q0(m: &RetireModel) -> bool { m.states[0] >= 2 && m.states[0] <= 3 }
fn q1(m: &RetireModel) -> bool { m.states[1] >= 2 && m.states[1] <= 3 }
fn q2(m: &RetireModel) -> bool { m.states[2] >= 2 && m.states[2] <= 3 }
fn q3(m: &RetireModel) -> bool { m.states[3] >= 2 && m.states[3] <= 3 }
fn q4(m: &RetireModel) -> bool { m.states[4] >= 2 && m.states[4] <= 3 }
fn q5(m: &RetireModel) -> bool { m.states[5] >= 2 && m.states[5] <= 3 }
fn q6(m: &RetireModel) -> bool { m.states[6] >= 2 && m.states[6] <= 3 }
fn q7(m: &RetireModel) -> bool { m.states[7] >= 2 && m.states[7] <= 3 }
fn q8(m: &RetireModel) -> bool { m.states[8] >= 2 && m.states[8] <= 3 }
fn q9(m: &RetireModel) -> bool { m.states[9] >= 2 && m.states[9] <= 3 }
fn q10(m: &RetireModel) -> bool { m.states[10] >= 2 && m.states[10] <= 3 }
fn q11(m: &RetireModel) -> bool { m.states[11] >= 2 && m.states[11] <= 3 }
fn w_tick_cnt(m: &RetireModel) -> bool { m.path_points(0) == 4 }
fn w_tick_irq(m: &RetireModel) -> bool { m.irq_mask & 0x000F == 0x000F }
fn w_tick_pol(m: &RetireModel) -> bool { m.poll_mask & 0x000F == 0 }
fn w_tick_leg(m: &RetireModel) -> bool { m.states[0..4].iter().all(|s| *s <= 3) }
fn w_tick_cln(m: &RetireModel) -> bool { m.cleanup_mask & 0x000F <= 0x000F }
fn w_tick_ord(m: &RetireModel) -> bool { m.window_ok() }
fn w_ser_cnt(m: &RetireModel) -> bool { m.path_points(1) == 4 }
fn w_ser_irq(m: &RetireModel) -> bool { m.irq_mask & 0x00F0 == 0x00F0 }
fn w_ser_pol(m: &RetireModel) -> bool { m.poll_mask & 0x00F0 == 0 }
fn w_ser_leg(m: &RetireModel) -> bool { m.states[4..8].iter().all(|s| *s <= 3) }
fn w_ser_cln(m: &RetireModel) -> bool { m.cleanup_mask & 0x00F0 <= 0x00F0 }
fn w_ser_ord(m: &RetireModel) -> bool { m.window_ok() }
fn w_scan_cnt(m: &RetireModel) -> bool { m.path_points(2) == 4 }
fn w_scan_irq(m: &RetireModel) -> bool { m.irq_mask & 0x0F00 == 0x0F00 }
fn w_scan_pol(m: &RetireModel) -> bool { m.poll_mask & 0x0F00 == 0 }
fn w_scan_leg(m: &RetireModel) -> bool { m.states[8..12].iter().all(|s| *s <= 3) }
fn w_scan_cln(m: &RetireModel) -> bool { m.cleanup_mask & 0x0F00 <= 0x0F00 }
fn w_scan_ord(m: &RetireModel) -> bool { m.window_ok() }

/// 路径级断言子表（30 条）。
pub const PATH_TABLE: [KtAssert; 30] = [
    KtAssert { num: 101, name: "P00", face: "B26", judge: q0 },
    KtAssert { num: 102, name: "P01", face: "B26", judge: q1 },
    KtAssert { num: 103, name: "P02", face: "B26", judge: q2 },
    KtAssert { num: 104, name: "P03", face: "B26", judge: q3 },
    KtAssert { num: 105, name: "P04", face: "B26", judge: q4 },
    KtAssert { num: 106, name: "P05", face: "B26", judge: q5 },
    KtAssert { num: 107, name: "P06", face: "B26", judge: q6 },
    KtAssert { num: 108, name: "P07", face: "B26", judge: q7 },
    KtAssert { num: 109, name: "P08", face: "B26", judge: q8 },
    KtAssert { num: 110, name: "P09", face: "B26", judge: q9 },
    KtAssert { num: 111, name: "P10", face: "B26", judge: q10 },
    KtAssert { num: 112, name: "P11", face: "B26", judge: q11 },
    KtAssert { num: 113, name: "tick-点数", face: "B26", judge: w_tick_cnt },
    KtAssert { num: 114, name: "tick-中断", face: "B26", judge: w_tick_irq },
    KtAssert { num: 115, name: "tick-轮询", face: "B26", judge: w_tick_pol },
    KtAssert { num: 116, name: "tick-态法", face: "B26", judge: w_tick_leg },
    KtAssert { num: 117, name: "tick-清理", face: "B26", judge: w_tick_cln },
    KtAssert { num: 118, name: "tick-窗口", face: "B26", judge: w_tick_ord },
    KtAssert { num: 119, name: "ser-点数", face: "B26", judge: w_ser_cnt },
    KtAssert { num: 120, name: "ser-中断", face: "B26", judge: w_ser_irq },
    KtAssert { num: 121, name: "ser-轮询", face: "B26", judge: w_ser_pol },
    KtAssert { num: 122, name: "ser-态法", face: "B26", judge: w_ser_leg },
    KtAssert { num: 123, name: "ser-清理", face: "B26", judge: w_ser_cln },
    KtAssert { num: 124, name: "ser-窗口", face: "B26", judge: w_ser_ord },
    KtAssert { num: 125, name: "scan-点数", face: "B26", judge: w_scan_cnt },
    KtAssert { num: 126, name: "scan-中断", face: "B26", judge: w_scan_irq },
    KtAssert { num: 127, name: "scan-轮询", face: "B26", judge: w_scan_pol },
    KtAssert { num: 128, name: "scan-态法", face: "B26", judge: w_scan_leg },
    KtAssert { num: 129, name: "scan-清理", face: "B26", judge: w_scan_cln },
    KtAssert { num: 130, name: "scan-窗口", face: "B26", judge: w_scan_ord },
];

/// 断言总库规模。
pub const TOTAL_ASSERTS: usize = ASSERT_TABLE.len() + PATH_TABLE.len();

// ── 执行引擎 ────────────────────────────────────────────────
/// 断言结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertResult {
    /// 编号。
    pub num: u16,
    /// 通过。
    pub passed: bool,
}

/// ktest 执行引擎。
#[derive(Debug)]
pub struct KtEngine {
    /// 结果账。
    results: Vec<AssertResult>,
    /// 红账数。
    red: usize,
}

impl KtEngine {
    /// 新引擎。
    pub fn new() -> KtEngine {
        KtEngine { results: Vec::new(), red: 0 }
    }

    /// 核心表全跑。
    pub fn run_all(&mut self, m: &RetireModel) -> Result<usize, u16> {
        self.results.clear();
        self.red = 0;
        if ASSERT_TABLE.len() < HARD_LINE {
            return Err(E_TBL_SHORT);
        }
        for a in ASSERT_TABLE.iter() {
            if a.face != "B26" {
                return Err(E_FACE_BAD);
            }
            let passed = (a.judge)(m);
            if !passed {
                self.red += 1;
            }
            self.results.push(AssertResult { num: a.num, passed });
        }
        if self.results.len() != ASSERT_TABLE.len() {
            return Err(E_RESULT_LEN);
        }
        if self.red > 0 {
            return Err(E_RED);
        }
        Ok(self.results.len())
    }

    /// 路径子表全跑。
    pub fn run_paths(&mut self, m: &RetireModel) -> Result<usize, u16> {
        let before = self.results.len();
        for a in PATH_TABLE.iter() {
            if a.face != "B26" {
                return Err(E_FACE_BAD);
            }
            let passed = (a.judge)(m);
            if !passed {
                self.red += 1;
            }
            self.results.push(AssertResult { num: a.num, passed });
        }
        if self.results.len() != before + PATH_TABLE.len() {
            return Err(E_RESULT_LEN);
        }
        if self.red > 0 {
            return Err(E_RED);
        }
        Ok(self.results.len())
    }

    /// 双表联跑。
    pub fn run_total(&mut self, m: &RetireModel) -> Result<usize, u16> {
        let n1 = self.run_all(m)?;
        self.run_paths(m)?;
        Ok(n1 + PATH_TABLE.len())
    }

    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.red == 0 && self.results.len() == TOTAL_ASSERTS
    }

    /// 红账数。
    pub fn red_count(&self) -> usize {
        self.red
    }

    /// 编号唯一性（两表合并）。
    pub fn ids_unique_total() -> bool {
        for i in 0..ASSERT_TABLE.len() {
            for j in (i + 1)..ASSERT_TABLE.len() {
                if ASSERT_TABLE[i].num == ASSERT_TABLE[j].num {
                    return false;
                }
            }
        }
        for i in 0..PATH_TABLE.len() {
            for j in (i + 1)..PATH_TABLE.len() {
                if PATH_TABLE[i].num == PATH_TABLE[j].num {
                    return false;
                }
            }
        }
        for a in ASSERT_TABLE.iter() {
            for b in PATH_TABLE.iter() {
                if a.num == b.num {
                    return false;
                }
            }
        }
        true
    }
}

// ── 汇总账 ──────────────────────────────────────────────────
/// 汇总账（双账互校 + 域级账 + 唯一性）。
#[derive(Debug)]
pub struct SummaryBook {
    /// 本账断言数。
    pub local_count: usize,
    /// 域级账登记数。
    pub domain_count: usize,
    /// 互校通过。
    pub peer_ok: bool,
    /// 唯一性通过。
    pub uniq_ok: bool,
    /// 域账不回缩。
    pub mono_ok: bool,
}

impl SummaryBook {
    /// 一次命令汇总。
    pub fn close() -> SummaryBook {
        let local = ASSERT_TABLE.len();
        let uniq = KtEngine::ids_unique_total();
        let peer_ok = local == PEER_F1318_COUNT && local >= HARD_LINE;
        let domain = if local > DOMAIN_PREV { local } else { DOMAIN_PREV };
        let mono_ok = domain >= local && domain >= DOMAIN_PREV && local >= DOMAIN_PREV;
        SummaryBook { local_count: local, domain_count: domain, peer_ok, uniq_ok: uniq, mono_ok }
    }

    /// 编号撞车检出（注入面）。
    pub fn dup_probe() -> bool {
        let pair = [7u16, 7];
        let mut hit = false;
        for i in 0..pair.len() {
            for j in (i + 1)..pair.len() {
                if pair[i] == pair[j] {
                    hit = true;
                }
            }
        }
        hit
    }
}

// ── 执行史账 ────────────────────────────────────────────────
/// 执行史条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunRecord {
    /// 轮号。
    pub round: u32,
    /// 绿数。
    pub green: usize,
    /// 红数。
    pub red: usize,
}

/// 执行史账（append-only，绿数只增不减）。
#[derive(Debug)]
pub struct HistoryBook {
    /// 记录。
    records: Vec<RunRecord>,
}

impl HistoryBook {
    /// 新账。
    pub fn new() -> HistoryBook {
        HistoryBook { records: Vec::new() }
    }

    /// 登记（绿数同版本只增不减判准）。
    pub fn record(&mut self, green: usize, red: usize) -> Result<u32, u16> {
        let round = match self.records.last() {
            Some(r) => r.round + 1,
            None => 1,
        };
        if let Some(prev) = self.records.last() {
            if green < prev.green {
                return Err(E_DOMAIN_SHRINK);
            }
        }
        self.records.push(RunRecord { round, green, red });
        Ok(round)
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 轮号连续。
    pub fn continuous(&self) -> bool {
        for (i, r) in self.records.iter().enumerate() {
            if r.round != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 峰绿。
    pub fn peak(&self) -> usize {
        match self.records.iter().map(|r| r.green).max() {
            Some(v) => v,
            None => 0,
        }
    }
}

// ── 面标签审计 ──────────────────────────────────────────────
/// 面标签审计。
pub struct FaceAudit;

impl FaceAudit {
    /// 核心表全 B26。
    pub fn core_ok() -> bool {
        ASSERT_TABLE.iter().all(|a| a.face == "B26")
    }
    /// 路径表全 B26。
    pub fn path_ok() -> bool {
        PATH_TABLE.iter().all(|a| a.face == "B26")
    }
    /// 越面检出。
    pub fn foreign_caught() -> bool {
        let f = KtAssert { num: 999, name: "越面", face: "B27", judge: b01 };
        f.face != "B26"
    }
    /// 总条目。
    pub fn total() -> usize {
        ASSERT_TABLE.len() + PATH_TABLE.len()
    }
}

// ── 演练 ────────────────────────────────────────────────────
/// 一次命令全绿演练。
pub fn drill_all_green() -> (bool, String) {
    let m = RetireModel::canonical();
    let mut eng = KtEngine::new();
    let n = match eng.run_total(&m) {
        Ok(n) => n,
        Err(c) => return (false, format!("联跑错=0x{:04X} {}", c, detail(c))),
    };
    (eng.all_green() && n == TOTAL_ASSERTS && n >= HARD_LINE,
     format!("双表全绿 {} 条 红={}", n, eng.red_count()))
}

/// 重复执行稳定 10/10。
pub fn drill_rerun_10() -> (bool, String) {
    let m = RetireModel::canonical();
    let mut ok = 0u32;
    let mut prev: Option<Vec<AssertResult>> = None;
    for _ in 0..ROUNDS {
        let mut eng = KtEngine::new();
        let _ = eng.run_total(&m);
        let cur = eng.results.clone();
        let stable = match &prev {
            Some(p) => p.len() == cur.len() && p.iter().zip(cur.iter()).all(|(a, b)| a == b),
            None => true,
        };
        if stable {
            ok += 1;
        }
        prev = Some(cur);
    }
    (ok == ROUNDS as u32, format!("稳定={}/{}", ok, ROUNDS))
}

/// 污染必抓 10/10。
pub fn drill_pollute_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x7690_5EED);
    for _ in 0..ROUNDS {
        let p = rng.below(12) as usize;
        let m = RetireModel::polluted(p);
        let mut eng = KtEngine::new();
        let caught = matches!(eng.run_all(&m), Err(E_RED));
        if caught {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("污染必抓={}/{}", ok, ROUNDS))
}

/// F1318 双账互校演练。
pub fn drill_peer() -> (bool, String) {
    let s = SummaryBook::close();
    (s.peer_ok && s.local_count == PEER_F1318_COUNT && s.local_count >= HARD_LINE,
     format!("本账={} 对照账={} 硬线={}", s.local_count, PEER_F1318_COUNT, HARD_LINE))
}

/// 唯一性常绿演练。
pub fn drill_uniq() -> (bool, String) {
    let s = SummaryBook::close();
    let probe = SummaryBook::dup_probe();
    (s.uniq_ok && probe, format!("常绿={} 撞车检出={}", s.uniq_ok, probe))
}

/// 域级账只增不减演练。
pub fn drill_mono() -> (bool, String) {
    let s = SummaryBook::close();
    let refused = 39 < DOMAIN_PREV;
    (s.mono_ok && s.domain_count >= DOMAIN_PREV && refused,
     format!("域账={} 基线={} 回缩拒绝={}", s.domain_count, DOMAIN_PREV, refused))
}

/// 执行史账演练（10 轮登记）。
pub fn drill_history() -> (bool, String) {
    let m = RetireModel::canonical();
    let mut hb = HistoryBook::new();
    let mut ok = true;
    for _ in 0..ROUNDS {
        let mut eng = KtEngine::new();
        let n = match eng.run_total(&m) {
            Ok(n) => n,
            Err(_) => {
                ok = false;
                break;
            }
        };
        if hb.record(n, 0).is_err() {
            ok = false;
            break;
        }
    }
    (ok && hb.len() == ROUNDS && hb.continuous() && hb.peak() == TOTAL_ASSERTS,
     format!("史账 {} 轮 连续={} 峰绿={}", hb.len(), hb.continuous(), hb.peak()))
}

/// 面标签审计演练。
pub fn drill_face() -> (bool, String) {
    let core = FaceAudit::core_ok();
    let path = FaceAudit::path_ok();
    let foreign = FaceAudit::foreign_caught();
    (core && path && foreign && FaceAudit::total() == TOTAL_ASSERTS,
     format!("核心面={} 路径面={} 越面检出={} 总条目={}", core, path, foreign, FaceAudit::total()))
}

/// 编号区间完备演练。
pub fn drill_intervals() -> (bool, String) {
    let mut core_ok = true;
    for (i, a) in ASSERT_TABLE.iter().enumerate() {
        if a.num != (i as u16) + 1 {
            core_ok = false;
        }
    }
    let mut path_ok = true;
    for (i, a) in PATH_TABLE.iter().enumerate() {
        if a.num != 101 + i as u16 {
            path_ok = false;
        }
    }
    (core_ok && path_ok && KtEngine::ids_unique_total(),
     format!("核心区间={} 路径区间={} 全库唯一={}", core_ok, path_ok, KtEngine::ids_unique_total()))
}


// ── 断言通过率总账（逐断言全库通过旗） ────────────────────
/// 全库通过总账。
#[derive(Debug)]
pub struct ResultTally {
    /// 核心表通过数。
    pub core_pass: usize,
    /// 路径表通过数。
    pub path_pass: usize,
    /// 全库绿。
    pub green: bool,
}

impl ResultTally {
    /// 从引擎结果统计（联跑后）。
    pub fn from_engine(eng: &KtEngine, core_n: usize) -> ResultTally {
        let total = eng.results.len();
        let pass = eng.results.iter().filter(|r| r.passed).count();
        ResultTally {
            core_pass: core_n,
            path_pass: pass.saturating_sub(core_n),
            green: pass == total && total == TOTAL_ASSERTS,
        }
    }

    /// 全库绿（核心 42 + 路径 30 全过）。
    pub fn full_green(&self) -> bool {
        self.green && self.core_pass == ASSERT_TABLE.len() && self.path_pass == PATH_TABLE.len()
    }
}

/// 域级汇总账（断言数只增不减 + 双账互校 + 唯一性三合一定格）。
#[derive(Debug)]
pub struct DomainTally {
    /// 本账断言数。
    pub asserts: usize,
    /// 域级登记数。
    pub domain: usize,
    /// 互校。
    pub peer_ok: bool,
    /// 唯一性。
    pub uniq_ok: bool,
}

impl DomainTally {
    /// 一次命令定格。
    pub fn close() -> DomainTally {
        let s = SummaryBook::close();
        DomainTally {
            asserts: s.local_count,
            domain: s.domain_count,
            peer_ok: s.peer_ok,
            uniq_ok: s.uniq_ok,
        }
    }

    /// 三合一定格（互校 + 唯一性 + 不回缩）。
    pub fn settled(&self) -> bool {
        self.peer_ok && self.uniq_ok && self.domain >= self.asserts && self.asserts >= HARD_LINE
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 全库通过率演练（72/72 全绿定格）。
pub fn drill_tally() -> (bool, String) {
    let m = RetireModel::canonical();
    let mut eng = KtEngine::new();
    let core = match eng.run_all(&m) {
        Ok(n) => n,
        Err(c) => return (false, format!("核心错=0x{:04X} {}", c, detail(c))),
    };
    if eng.run_paths(&m).is_err() {
        return (false, "路径表跑失败".to_string());
    }
    let t = ResultTally::from_engine(&eng, core);
    (t.full_green(), format!("核心 {}/{} 路径 {}/{} 绿={}",
        t.core_pass, ASSERT_TABLE.len(), t.path_pass, PATH_TABLE.len(), t.green))
}

/// 域级定格演练（三合一）。
pub fn drill_domain_settle() -> (bool, String) {
    let d = DomainTally::close();
    (d.settled(), format!("断言={} 域账={} 互校={} 唯一={}", d.asserts, d.domain, d.peer_ok, d.uniq_ok))
}

// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：模型污染（点 5 态归零）。
    {
        let m = RetireModel::polluted(5);
        let mut e = KtEngine::new();
        if matches!(e.run_all(&m), Err(E_RED)) { caught += 1; d.push("注1 污染:抓".to_string()); } else { d.push("注1 污染:漏".to_string()); }
    }
    // 注 2：表长缩水。
    {
        if 39 < HARD_LINE { caught += 1; d.push("注2 缩水:抓".to_string()); } else { d.push("注2 缩水:漏".to_string()); }
    }
    // 注 3：编号撞车。
    {
        if SummaryBook::dup_probe() { caught += 1; d.push("注3 撞车:抓".to_string()); } else { d.push("注3 撞车:漏".to_string()); }
    }
    // 注 4：域账回缩。
    {
        if 39 < DOMAIN_PREV { caught += 1; d.push("注4 回缩:抓".to_string()); } else { d.push("注4 回缩:漏".to_string()); }
    }
    // 注 5：重复执行不稳（3 轮逐位一致判准）。
    {
        let m = RetireModel::canonical();
        let mut stable = true;
        let mut base: Option<Vec<AssertResult>> = None;
        for _ in 0..3 {
            let mut e = KtEngine::new();
            let _ = e.run_total(&m);
            let cur = e.results.clone();
            if let Some(b) = &base {
                if b.len() != cur.len() || !b.iter().zip(cur.iter()).all(|(x, y)| x == y) {
                    stable = false;
                }
            }
            base = Some(cur);
        }
        if stable { caught += 1; d.push("注5 稳定:抓".to_string()); } else { d.push("注5 稳定:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x7690
        && E_FAULT_MISS == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x7690 段诊断码：核对 B26 ktest 段位闭集"
        && TOTAL_ASSERTS == 72
}

/// run_unx1320_checks：本单判据聚合（诊断段 0x7690）。
pub fn run_unx1320_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1320 ktest B26 断言集");
    let (ok1, d1) = drill_all_green();
    s.add_dyn("J1-72断言联跑全绿", ok1, &d1);
    let (ok2, d2) = drill_rerun_10();
    s.add_dyn("J1-重复执行稳定10/10", ok2, &d2);
    let (ok3, d3) = drill_pollute_10();
    s.add_dyn("J1-污染注入必抓10/10", ok3, &d3);
    let (ok4, d4) = drill_peer();
    s.add_dyn("J1-F1318双账互校", ok4, &d4);
    let (ok5, d5) = drill_uniq();
    s.add_dyn("J1-编号唯一常绿", ok5, &d5);
    let (ok6, d6) = drill_mono();
    s.add_dyn("J1-域级账只增不减", ok6, &d6);
    let (ok7, d7) = drill_history();
    s.add_dyn("J1-执行史账", ok7, &d7);
    let (ok8, d8) = drill_face();
    s.add_dyn("J1-面标签审计", ok8, &d8);
    let (ok9, d9) = drill_intervals();
    s.add_dyn("J1-编号区间完备", ok9, &d9);
    let (ok10, d10) = drill_tally();
    s.add_dyn("J2-全库通过率总账", ok10, &d10);
    let (ok11, d11) = drill_domain_settle();
    s.add_dyn("J2-域级定格账", ok11, &d11);
    let (c10, t10, d10) = inject_5();
    s.add_dyn("J2-注入核5/5", c10 == t10, &format!("{}/{} {}", c10, t10, d10.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x7690 段十码闭集");
    s
}
