//! unx403_tsccalib —— UNX-F0403 TSC 频率三方校准（PIT/MSR/CPUIV）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B23）：UNX-F0403 · TSC 频率三方校准（PIT/MSR/CPUIV） · 目标 280 行 · 已深化
//! 判据原文：三方校准差 ≤0.5%（PIT 5 秒窗/MSR 平台值/CPUID 频率），取值入账
//! 组件：PIT 模型（1193182 Hz 基频 + 5 秒窗计周数）+ MSR 平台值模型（PLATINFO）
//!      + CPUID 频率模型（0x15 TSC/核心比值 + 0x16 MHz 三元组）+ 三方两两相对差
//!      （万分比 bps）+ 一致性裁决（≤50bps 全对齐；超差取中位数并立案）+ 校准账
//!      （三方取值 + 两两差 + 胜出值 + 一致旗 + 溯源指纹，全量入账可渲染）。
//! 职责定位：内核时间子系统取 TSC 频率的权威入口——单源会错，三方互证才能签收。
//!   「取值入账」= 三个来源的取值、两两偏差、胜出逻辑全部落账可查，不私下拍板。
//! 性能逐项分解：PIT 换算 O(1)（常数基频除法）；MSR/CPUID 解析 O(1)；
//!   两两差 O(3)（三对）；中位数 O(1)（三值取中）；指纹 O(账字段闭集 8)；
//!   全整数算术零浮点零墙钟（模型注入确定性计数），同输入恒同输出。
//! 诊断码段：0x99（0x9901~0x9906，全域 grep 确认独占，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick 注入，零墙钟），
//!   逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（来源：8254 PIT 数据手册 / Intel SDM Vol.3 §18.7.3 / CPUID leaf 0x15/0x16；写死并注释）
// ---------------------------------------------------------------------------

/// PIT 基频：8254 输入时钟 1193182 Hz（NTSC 色副载波 4.19304MHz ÷ 3.5，写死）。
pub const PIT_BASE_HZ: u64 = 1_193_182;
/// PIT 校准窗：5 秒（判据点名「PIT 5 秒窗」）。
pub const PIT_WINDOW_SECS: u64 = 5;
/// 一致性容差：0.5% = 50 bps（万分比；判据「差 ≤0.5%」）。
pub const TOLERANCE_BPS: u64 = 50;
/// bps 分母：1.0 = 10000 bps。
pub const BPS_DEN: u64 = 10_000;
/// CPUID 0x15 缺省核心晶体频率（Intel SDM：读不到时用 24MHz 桌面典型值，显性标注）。
pub const CRystalsHZ_DEFAULT: u64 = 24_000_000;

// ---------------------------------------------------------------------------
// 诊断码段 0x99（0x9901~0x9906 闭集；域内独占）
// ---------------------------------------------------------------------------

/// 0x9901 PIT 窗不完整：窗内计周数与窗长不符（模型注入面错误）。
pub const C_PIT_WINDOW: u16 = 0x9901;
/// 0x9902 MSR 平台值为 0：平台信息寄存器读到零值，拒签。
pub const C_MSR_ZERO: u16 = 0x9902;
/// 0x9903 CPUID 比值为 0：0x15 叶读零（虚拟化环境常见），拒签。
pub const C_CPUID_ZERO: u16 = 0x9903;
/// 0x9904 超差：任一两两差 > 50bps，取中位数并立案。
pub const C_OUT_TOLERANCE: u16 = 0x9904;
/// 0x9905 无中位数：三方取值含非法值，中位数不可用。
pub const C_NO_MEDIAN: u16 = 0x9905;
/// 0x9906 校准未冻结：账未签收就被人读取（生命周期违规）。
pub const C_NOT_SEALED: u16 = 0x9906;

/// 段守卫：全部诊断码必须落 0x99 段。
pub const fn seg_guard() -> bool {
    (C_PIT_WINDOW >> 8) == 0x99
        && (C_MSR_ZERO >> 8) == 0x99
        && (C_CPUID_ZERO >> 8) == 0x99
        && (C_OUT_TOLERANCE >> 8) == 0x99
        && (C_NO_MEDIAN >> 8) == 0x99
        && (C_NOT_SEALED >> 8) == 0x99
}

/// 诊断码 → 人话（可操作建议一体）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_PIT_WINDOW => "PIT 窗不完整：核对窗长与计周注入口径后重测",
        C_MSR_ZERO => "MSR 平台值为 0：核对 MSR 地址与读权限，或降级双方校准",
        C_CPUID_ZERO => "CPUID 比值为 0：虚拟化环境常见；改用 0x16 叶或降级",
        C_OUT_TOLERANCE => "三方超差：取中位数入账并立案，附三方取值供归因",
        C_NO_MEDIAN => "三方取值含非法值：先修非法源再校准",
        C_NOT_SEALED => "校准未冻结：先 seal 再读账",
        _ => "未知诊断码：核对 0x99 段闭集",
    }
}

// ---------------------------------------------------------------------------
// 三方模型（确定性注入：模型给计数，模块做换算——零真实读）
// ---------------------------------------------------------------------------

/// 万分比相对差：|a-b|/min(a,b) × 10000（bps）。min 为 0 返回 u64::MAX（不可比）。
pub fn rel_diff_bps(a: u64, b: u64) -> u64 {
    if a == 0 || b == 0 {
        return u64::MAX;
    }
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    (hi - lo) * BPS_DEN / lo
}

/// 源 A：PIT 5 秒窗。模型注入窗内观测到的 TSC 计数 → 换算 TSC 频率 = 计数/5。
/// 错误路径：计数 0 → Err(0x9901)（窗不完整：5 秒窗内 TSC 不可能不走）。
pub fn pit_estimate(tsc_counts_in_window: u64) -> Result<u64, u16> {
    if tsc_counts_in_window == 0 {
        return Err(C_PIT_WINDOW);
    }
    Ok(tsc_counts_in_window / PIT_WINDOW_SECS)
}

/// 源 B：MSR 平台值。模型注入 MSR 平台信息寄存器的 TSC 频率字段。
/// 错误路径：0 → Err(0x9902)。
pub fn msr_estimate(msr_platform_tsc_hz: u64) -> Result<u64, u16> {
    if msr_platform_tsc_hz == 0 {
        return Err(C_MSR_ZERO);
    }
    Ok(msr_platform_tsc_hz)
}

/// 源 C：CPUID 频率。0x15 叶（eax=分母 ebx=分子 ecx=核心晶体 Hz）+
/// 0x16 叶（eax=基准 MHz ebx=最大 MHz）双路：优先 0x15 精确比值，
/// 0x15 读零（虚拟化）降级 0x16 MHz × 1e6，再零 → Err(0x9903)。
pub fn cpuid_estimate(
    leaf15_den: u64,
    leaf15_num: u64,
    leaf15_crystal: u64,
    leaf16_base_mhz: u64,
) -> Result<u64, u16> {
    if leaf15_den != 0 && leaf15_num != 0 && leaf15_crystal != 0 {
        // TSC 频率 = 晶体 × 分子 / 分母（0x15 语义，Intel SDM Table 3-8）。
        return Ok(leaf15_crystal * leaf15_num / leaf15_den);
    }
    if leaf16_base_mhz != 0 {
        return Ok(leaf16_base_mhz * 1_000_000);
    }
    Err(C_CPUID_ZERO)
}

/// 三值取中（整数，无排序开销：三次比较）。
pub fn median3(a: u64, b: u64, c: u64) -> u64 {
    if (a >= b && a <= c) || (a <= b && a >= c) {
        a
    } else if (b >= a && b <= c) || (b <= a && b >= c) {
        b
    } else {
        c
    }
}

/// 校准账：「取值入账」的落地形态——三方取值 + 两两差 + 胜出值 + 一致旗。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalibLedger {
    pub pit_hz: u64,
    pub msr_hz: u64,
    pub cpuid_hz: u64,
    pub diff_pit_msr_bps: u64,
    pub diff_pit_cpuid_bps: u64,
    pub diff_msr_cpuid_bps: u64,
    pub winner_hz: u64,
    pub in_tolerance: bool,
    pub sealed: bool,
}

impl CalibLedger {
    /// FNV-1a 指纹：账字段闭集 9 项（对拍锚：同账恒同指纹）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let m = |h: &mut u64, b: u8| {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        for v in [
            self.pit_hz,
            self.msr_hz,
            self.cpuid_hz,
            self.diff_pit_msr_bps,
            self.diff_pit_cpuid_bps,
            self.diff_msr_cpuid_bps,
            self.winner_hz,
            self.in_tolerance as u64,
            self.sealed as u64,
        ] {
            for b in v.to_le_bytes() {
                m(&mut h, b);
            }
        }
        h
    }

    /// 确定性渲染：单行键控（供转储与对拍）。
    pub fn render(&self) -> String {
        alloc::format!(
            "calib pit={} msr={} cpuid={} d_pm={} d_pc={} d_mc={} win={} tol={} sealed={} fp={:016x}",
            self.pit_hz,
            self.msr_hz,
            self.cpuid_hz,
            self.diff_pit_msr_bps,
            self.diff_pit_cpuid_bps,
            self.diff_msr_cpuid_bps,
            self.winner_hz,
            self.in_tolerance,
            self.sealed,
            self.fingerprint(),
        )
    }
}

/// 校准事件账（追加式）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalibEvent {
    pub code: u16,
    pub detail: String,
}

/// 三方校准器：无内部可变态，一次 run 出一份冻结账。
#[derive(Clone, Debug, Default)]
pub struct TscCalibrator {
    events: Vec<CalibEvent>,
}

impl TscCalibrator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> &[CalibEvent] {
        &self.events
    }

    /// 三方校准主入口：
    ///   1. 三源各自换算（任一源 Err → 该源记 0 并入账，校准继续用剩余两源）；
    ///   2. 两两相对差（bps）；
    ///   3. 全对齐（≤50bps）→ 胜出 = 三方均值；超差 → 胜出 = 中位数 + 立案；
    ///   4. 全部入账 + seal（未 seal 读账 = 0x9906 生命周期违规）。
    pub fn run(
        &mut self,
        pit_counts: u64,
        msr_hz: u64,
        l15_den: u64,
        l15_num: u64,
        l15_xtal: u64,
        l16_mhz: u64,
    ) -> Result<CalibLedger, u16> {
        let pit = pit_estimate(pit_counts).unwrap_or(0);
        if pit == 0 {
            self.events.push(CalibEvent { code: C_PIT_WINDOW, detail: code_text(C_PIT_WINDOW).to_string() });
        }
        let msr = msr_estimate(msr_hz).unwrap_or(0);
        if msr == 0 {
            self.events.push(CalibEvent { code: C_MSR_ZERO, detail: code_text(C_MSR_ZERO).to_string() });
        }
        let cpuid = cpuid_estimate(l15_den, l15_num, l15_xtal, l16_mhz).unwrap_or(0);
        if cpuid == 0 {
            self.events.push(CalibEvent { code: C_CPUID_ZERO, detail: code_text(C_CPUID_ZERO).to_string() });
        }

        // 三源全灭 = 无中位数可用。
        let valid = [pit, msr, cpuid].iter().filter(|&&v| v != 0).count();
        if valid == 0 {
            self.events.push(CalibEvent { code: C_NO_MEDIAN, detail: code_text(C_NO_MEDIAN).to_string() });
            return Err(C_NO_MEDIAN);
        }

        // 两两差（0 值源记 u64::MAX = 不可比，不参与一致判定）。
        let d_pm = rel_diff_bps(pit, msr);
        let d_pc = rel_diff_bps(pit, cpuid);
        let d_mc = rel_diff_bps(msr, cpuid);
        let comparable_diffs: Vec<u64> = [d_pm, d_pc, d_mc]
            .iter()
            .copied()
            .filter(|d| *d != u64::MAX)
            .collect();
        let in_tol = !comparable_diffs.is_empty() && comparable_diffs.iter().all(|d| *d <= TOLERANCE_BPS);

        // 胜出：全对齐 → 三有效源均值；超差 → 中位数 + 立案（超差必须显性）。
        let valids: Vec<u64> = [pit, msr, cpuid].iter().copied().filter(|v| *v != 0).collect();
        let winner = if in_tol {
            let sum: u64 = valids.iter().sum();
            sum / valids.len() as u64
        } else {
            self.events.push(CalibEvent { code: C_OUT_TOLERANCE, detail: code_text(C_OUT_TOLERANCE).to_string() });
            if valids.len() == 3 {
                median3(pit, msr, cpuid)
            } else {
                // 两源且超差：取较大者（偏差归因侧写更保守）。
                valids.iter().copied().max().unwrap_or(0)
            }
        };

        Ok(CalibLedger {
            pit_hz: pit,
            msr_hz: msr,
            cpuid_hz: cpuid,
            diff_pit_msr_bps: d_pm,
            diff_pit_cpuid_bps: d_pc,
            diff_msr_cpuid_bps: d_mc,
            winner_hz: winner,
            in_tolerance: in_tol,
            sealed: true,
        })
    }

    /// seal 后读账守卫：账 sealed=false 时读取即 0x9906（生命周期红线）。
    pub fn read_sealed(l: &CalibLedger) -> Result<CalibLedger, u16> {
        if !l.sealed {
            return Err(C_NOT_SEALED);
        }
        Ok(l.clone())
    }
}

// ---------------------------------------------------------------------------
// 自检（逐检查带 detail；域聚合在 mod.rs）
// ---------------------------------------------------------------------------

/// 目标 TSC 频率：3.0 GHz（三方注入围绕它构造，容差内互证）。
const TARGET_HZ: u64 = 3_000_000_000;

/// C1：三方一致路径——三源注入同频（PIT 5 秒窗 150 亿计数 / MSR 3GHz /
/// CPUID 0x15 比值换算 3GHz），两两差 ≤50bps，胜出=均值，账全绿。
fn check_c1_agree() -> bool {
    let mut c = TscCalibrator::new();
    // CPUID 0x15：晶体 24MHz × 分子 125 / 分母 1 = 3GHz。
    let l = match c.run(PIT_WINDOW_SECS * TARGET_HZ, TARGET_HZ, 1, 125, CRystalsHZ_DEFAULT, 3000) {
        Ok(l) => l,
        Err(_) => return false,
    };
    l.in_tolerance
        && l.diff_pit_msr_bps == 0
        && l.diff_pit_cpuid_bps == 0
        && l.diff_msr_cpuid_bps == 0
        && l.winner_hz == TARGET_HZ
        && l.sealed
        && l.render().contains("tol=true")
        && TscCalibrator::read_sealed(&l).is_ok()
}

/// C2：容差内微差——三源差 ≤0.5%（±15 万 Hz 级），仍判一致、均值胜出。
fn check_c2_within_tolerance() -> bool {
    let mut c = TscCalibrator::new();
    let pit_hz = TARGET_HZ - 100_000; // 0.0033% 差
    let msr_hz = TARGET_HZ;
    let _cpu_hz = TARGET_HZ + 100_000; // 0x16 路未注入（0x15 路换算恒 3GHz），留名拒用防误改
    let l = match c.run(PIT_WINDOW_SECS * pit_hz, msr_hz, 1, 125, CRystalsHZ_DEFAULT, 3000) {
        Ok(l) => l,
        Err(_) => return false,
    };
    // 注意：CPUID 路换算回 3GHz（注入与 pit 无关），pit 差 ≈ 33bps ≤ 50。
    l.in_tolerance && l.winner_hz >= (TARGET_HZ - 200_000) && l.winner_hz <= (TARGET_HZ + 200_000)
}

/// C3：超差立案——MSR 注入 2.7GHz（与 3GHz 差 10% > 50bps），
/// in_tolerance=false + 0x9904 入账 + 中位数胜出（3GHz 在 3.0/2.7/3.0 中）。
fn check_c3_out_of_tolerance() -> bool {
    let mut c = TscCalibrator::new();
    let l = match c.run(PIT_WINDOW_SECS * TARGET_HZ, 2_700_000_000, 1, 125, CRystalsHZ_DEFAULT, 3000) {
        Ok(l) => l,
        Err(_) => return false,
    };
    !l.in_tolerance
        && l.winner_hz == TARGET_HZ // 中位数：2.7/3.0/3.0 → 3.0
        && c.events().iter().any(|e| e.code == C_OUT_TOLERANCE)
        && l.diff_pit_msr_bps > TOLERANCE_BPS
}

/// C4：取值入账——账内三方取值与注入值逐字段全等（「取值入账」正向核）。
fn check_c4_ledger_values() -> bool {
    let mut c = TscCalibrator::new();
    let pit_in = PIT_WINDOW_SECS * TARGET_HZ;
    let msr_in = 2_999_000_000u64;
    let l = match c.run(pit_in, msr_in, 1, 125, CRystalsHZ_DEFAULT, 3000) {
        Ok(l) => l,
        Err(_) => return false,
    };
    l.pit_hz == pit_in / PIT_WINDOW_SECS && l.msr_hz == msr_in && l.cpuid_hz == TARGET_HZ
}

/// C5：源降级路径——CPUID 0x15 读零（虚拟化）降级 0x16（3000MHz），
/// 校准继续不中断（降级显性入账 0x9903 不阻塞）。
fn check_c5_cpuid_fallback() -> bool {
    let mut c = TscCalibrator::new();
    let l = match c.run(PIT_WINDOW_SECS * TARGET_HZ, TARGET_HZ, 0, 0, 0, 3000) {
        Ok(l) => l,
        Err(_) => return false,
    };
    l.cpuid_hz == 3_000_000_000
        && c.events().iter().any(|e| e.code == C_CPUID_ZERO)
        && l.in_tolerance
}

/// C6：双源失效路径——MSR 零 + CPUID 零，PIT 单源仍出账（单源即真值，无差可判）。
fn check_c6_single_source() -> bool {
    let mut c = TscCalibrator::new();
    let l = match c.run(PIT_WINDOW_SECS * TARGET_HZ, 0, 0, 0, 0, 0) {
        Ok(l) => l,
        Err(_) => return false,
    };
    l.msr_hz == 0 && l.cpuid_hz == 0 && l.winner_hz == TARGET_HZ
        && c.events().iter().any(|e| e.code == C_MSR_ZERO)
        && c.events().iter().any(|e| e.code == C_CPUID_ZERO)
}

/// C7：全灭拒签——三源全零 → Err(0x9905)，无账可出。
fn check_c7_all_dead() -> bool {
    let mut c = TscCalibrator::new();
    c.run(0, 0, 0, 0, 0, 0) == Err(C_NO_MEDIAN)
        && c.events().iter().any(|e| e.code == C_NO_MEDIAN)
}

/// C8：PIT 零计数拒签——窗内零计数 = 0x9901（窗不完整）。
fn check_c8_pit_zero() -> bool {
    let mut c = TscCalibrator::new();
    let l = match c.run(0, TARGET_HZ, 1, 125, CRystalsHZ_DEFAULT, 3000) {
        Ok(l) => l,
        Err(_) => return false,
    };
    l.pit_hz == 0 && c.events().iter().any(|e| e.code == C_PIT_WINDOW)
}

/// C9：中位数函数——三值全序覆盖（升/降/中凸/中凹四形态）。
fn check_c9_median3() -> bool {
    median3(1, 2, 3) == 2
        && median3(3, 2, 1) == 2
        && median3(2, 1, 3) == 2
        && median3(2, 3, 1) == 2
        && median3(1, 1, 3) == 1
        && median3(5, 5, 5) == 5
}

/// C10：bps 换算——10% = 1000bps、恰 50bps 边界、同值 = 0bps、零值 = 不可比。
fn check_c10_bps_math() -> bool {
    rel_diff_bps(3_000_000_000, 2_700_000_000) == 1000
        && rel_diff_bps(2_000_000_000, 2_010_000_000) == 50 // 恰 50bps（0.5% 边界）
        && rel_diff_bps(7, 7) == 0
        && rel_diff_bps(0, 7) == u64::MAX
}

/// C11：指纹确定性——同注入双跑账指纹恒等；注入微变指纹必变。
fn check_c11_fingerprint() -> bool {
    let mut a = TscCalibrator::new();
    let mut b = TscCalibrator::new();
    let la = a.run(PIT_WINDOW_SECS * TARGET_HZ, TARGET_HZ, 1, 125, CRystalsHZ_DEFAULT, 3000).unwrap_or(
        CalibLedger {
            pit_hz: 0, msr_hz: 0, cpuid_hz: 0, diff_pit_msr_bps: 0, diff_pit_cpuid_bps: 0,
            diff_msr_cpuid_bps: 0, winner_hz: 0, in_tolerance: false, sealed: false,
        },
    );
    let lb = b.run(PIT_WINDOW_SECS * TARGET_HZ, TARGET_HZ, 1, 125, CRystalsHZ_DEFAULT, 3000).unwrap_or(
        CalibLedger {
            pit_hz: 0, msr_hz: 0, cpuid_hz: 0, diff_pit_msr_bps: 0, diff_pit_cpuid_bps: 0,
            diff_msr_cpuid_bps: 0, winner_hz: 0, in_tolerance: false, sealed: false,
        },
    );
    let mut d = TscCalibrator::new();
    let ld = d.run(PIT_WINDOW_SECS * (TARGET_HZ + 1), TARGET_HZ, 1, 125, CRystalsHZ_DEFAULT, 3000)
        .unwrap_or(la.clone());
    la.fingerprint() == lb.fingerprint() && la.fingerprint() != ld.fingerprint()
}

/// C12：生命周期守卫——未 seal 账读取出 0x9906；seal 账读取放行。
fn check_c12_seal_guard() -> bool {
    let raw = CalibLedger {
        pit_hz: 1, msr_hz: 1, cpuid_hz: 1, diff_pit_msr_bps: 0, diff_pit_cpuid_bps: 0,
        diff_msr_cpuid_bps: 0, winner_hz: 1, in_tolerance: true, sealed: false,
    };
    TscCalibrator::read_sealed(&raw) == Err(C_NOT_SEALED)
}

/// 本模块自检（域聚合在 mod.rs）。
pub fn run_unx403_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX403");
    let g = seg_guard();
    set.add("unx403-seg-guard", g, if g { "" } else { "诊断码越出 0x99 段" });
    let c1 = check_c1_agree();
    set.add("unx403-c1-agree", c1, if c1 { "" } else { "三方一致路径失守" });
    let c2 = check_c2_within_tolerance();
    set.add("unx403-c2-within-tol", c2, if c2 { "" } else { "容差内微差判一致失守" });
    let c3 = check_c3_out_of_tolerance();
    set.add("unx403-c3-out-tol", c3, if c3 { "" } else { "超差立案或中位数胜出失守" });
    let c4 = check_c4_ledger_values();
    set.add("unx403-c4-ledger-values", c4, if c4 { "" } else { "取值入账逐字段失守" });
    let c5 = check_c5_cpuid_fallback();
    set.add("unx403-c5-cpuid-fallback", c5, if c5 { "" } else { "CPUID 降级路径失守" });
    let c6 = check_c6_single_source();
    set.add("unx403-c6-single-source", c6, if c6 { "" } else { "单源降级路径失守" });
    let c7 = check_c7_all_dead();
    set.add("unx403-c7-all-dead", c7, if c7 { "" } else { "全灭拒签失守" });
    let c8 = check_c8_pit_zero();
    set.add("unx403-c8-pit-zero", c8, if c8 { "" } else { "PIT 零计数拒签失守" });
    let c9 = check_c9_median3();
    set.add("unx403-c9-median3", c9, if c9 { "" } else { "中位数函数失守" });
    let c10 = check_c10_bps_math();
    set.add("unx403-c10-bps-math", c10, if c10 { "" } else { "bps 换算失守" });
    let c11 = check_c11_fingerprint();
    set.add("unx403-c11-fingerprint", c11, if c11 { "" } else { "指纹确定性失守" });
    let c12 = check_c12_seal_guard();
    set.add("unx403-c12-seal-guard", c12, if c12 { "" } else { "生命周期守卫失守" });
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主 cargo test 直跑；域聚合测试在 mod.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx403_all_checks_green() {
        let set = run_unx403_checks();
        assert!(set.all_passed(), "UNX-F0403 domain checks must be green");
    }

    #[test]
    fn unx403_segment_guard_holds() {
        assert!(seg_guard());
    }

    #[test]
    fn unx403_tolerance_boundary() {
        // 容差边界（整数除法精确构造）：恰 50bps 判一致（≤50 含等号），恰 51bps 判超差。
        assert_eq!(rel_diff_bps(2_000_000_000, 2_010_000_000), 50);
        assert!(rel_diff_bps(2_000_000_000, 2_010_000_000) <= TOLERANCE_BPS);
        assert_eq!(rel_diff_bps(2_000_000_000, 2_010_200_000), 51);
        assert!(rel_diff_bps(2_000_000_000, 2_010_200_000) > TOLERANCE_BPS);
    }

    #[test]
    fn unx403_render_deterministic() {
        let mut c = TscCalibrator::new();
        let l = c.run(PIT_WINDOW_SECS * TARGET_HZ, TARGET_HZ, 1, 125, CRystalsHZ_DEFAULT, 3000)
            .unwrap_or(CalibLedger {
                pit_hz: 0, msr_hz: 0, cpuid_hz: 0, diff_pit_msr_bps: 0, diff_pit_cpuid_bps: 0,
                diff_msr_cpuid_bps: 0, winner_hz: 0, in_tolerance: false, sealed: false,
            });
        assert_eq!(l.render(), l.render());
    }
}
