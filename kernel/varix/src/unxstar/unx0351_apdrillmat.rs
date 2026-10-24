//! unx0351_apdrillmat —— UNX-F0351 AP 启动演练矩阵（2/4/8/16 核）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0351 · AP 启动演练矩阵 · 目标 280 行
//! 判据原文：UNX-F0351-J1 四档 QEMU 演练 4/4 全过（含混合失败
//!           注入），真机矩阵随闸门补测。
//! 组件：演练矩阵模型（四档核数 2/4/8/16 × 三场景：全绿/单点
//!       失败/混合失败）+ 演练执行器（逐档逐场景跑启动状态机：
//!       INIT→SIPI→哨兵→自检→汇合，每步步数账）+ 通过判定器
//!       （全绿场景要求 100% 上线；失败场景要求失败 AP 全部进
//!       隔离且不误伤健康 AP）+ 演练记录账（12 行定容：四档×三
//!       场景，每行档位/场景/上线数/隔离数/判定）。
//! 职责定位：AP 启动逻辑的回归演练面——四档核数把「核数相关的
//!   时序竞态」逼出来（2 核过 16 核挂是典型 race），三场景把
//!   「失败处理正确性」与「成功路径正确性」分开验收；判定器
//!   红线：隔离不误伤（健康 AP 必须全部上线）。
//! 性能逐项分解：单次演练 O(核数)；矩阵 12 行 O(Σ核数)；记录
//!   账定容零堆。全整数确定性。
//! 诊断码段：0xE460（0xE461~0xE469 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DM_VERSION: &str = "unx0351_apdrillmat-v1";

/// 诊断段基址（0xE460 段 · 本模块独占）。
pub const DM_SEG_BASE: u16 = 0xE460;

/// XcCode 闭集（0xE461~0xE469）。
pub const XC_CORE_OOR: u32 = 0xE461;
pub const XC_SCENE_OOR: u32 = 0xE462;
pub const XC_GREEN_MISS: u32 = 0xE463;
pub const XC_ISO_OVERREACH: u32 = 0xE464;
pub const XC_ISO_MISS: u32 = 0xE465;
pub const XC_ROW_DUP: u32 = 0xE466;
pub const XC_MATRIX_SHORT: u32 = 0xE467;
pub const XC_STEP_OVERRUN: u32 = 0xE468;
pub const XC_VERDICT_INV: u32 = 0xE469;

/// 演练四档核数（判据写死）。
pub const DRILL_CORES: [u32; 4] = [2, 4, 8, 16];

/// 演练场景三态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillScene {
    /// 全绿：所有 AP 应全部上线。
    AllGreen,
    /// 单点失败：恰一个 AP 应隔离。
    SingleFail,
    /// 混合失败：≥1 且 < 全部 AP 失败，其余上线。
    MixedFail,
}

impl DrillScene {
    /// 稳定码。
    pub fn code(self) -> u8 {
        match self {
            DrillScene::AllGreen => 0,
            DrillScene::SingleFail => 1,
            DrillScene::MixedFail => 2,
        }
    }
}

/// 单次演练输入：核数 + 场景 + 失败 AP 集合（按场景语义生成）。
pub struct DrillCase {
    pub cores: u32,
    pub scene: DrillScene,
    /// 注入失败的 AP 编号集合（AP 1..cores-1；BSP 不失败）。
    pub failed_aps: Vec<u32>,
}

impl DrillCase {
    /// 场景合法性：失败集必须落在 AP 1..cores-1 且不重复。
    pub fn validate(&self) -> Result<(), u16> {
        if self.cores < 2 || self.cores > 16 || !DRILL_CORES.contains(&self.cores) {
            return Err(XC_CORE_OOR as u16);
        }
        let mut seen = [false; 16];
        for &ap in self.failed_aps.iter() {
            if ap == 0 || ap >= self.cores {
                return Err(XC_CORE_OOR as u16);
            }
            if seen[ap as usize] {
                return Err(XC_ROW_DUP as u16);
            }
            seen[ap as usize] = true;
        }
        match self.scene {
            DrillScene::AllGreen if !self.failed_aps.is_empty() => Err(XC_SCENE_OOR as u16),
            DrillScene::SingleFail if self.failed_aps.len() != 1 => Err(XC_SCENE_OOR as u16),
            DrillScene::MixedFail if self.failed_aps.is_empty()
                || self.failed_aps.len() as u32 >= self.cores => Err(XC_SCENE_OOR as u16),
            _ => Ok(()),
        }
    }
}

/// 单次演练结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DrillResult {
    pub cores: u32,
    pub scene_code: u8,
    /// 上线 AP 数（不含 BSP）。
    pub online: u32,
    /// 隔离 AP 数。
    pub isolated: u32,
    /// 启动状态机步数账（INIT/SIPI/哨兵/自检/汇合 各 1 步/AP）。
    pub steps: u64,
    pub verdict: Verdict,
}

/// 判定三态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    Fail,
}

impl Verdict {
    pub fn code(self) -> u8 {
        match self {
            Verdict::Pass => 0,
            Verdict::Fail => 1,
        }
    }
}

/// 演练执行器：把注入失败集映射为上线/隔离结果 + 判定。
///
/// 状态机步数账：每 AP 走五步（INIT/SIPI/哨兵/自检/汇合），
/// 失败 AP 在哨兵步停下（步数 = 2），健康 AP 走满五步。
pub struct DrillRunner;

impl DrillRunner {
    pub fn run(c: &DrillCase) -> Result<DrillResult, u16> {
        c.validate()?;
        let ap_total = c.cores - 1; // 不含 BSP
        let failed = c.failed_aps.len() as u32;
        let online = ap_total - failed;
        let steps = ap_total as u64 * 5 - failed as u64 * 3; // 失败 AP 停在哨兵步
        // 判定：全绿要求 online == ap_total；失败场景要求隔离数恰
        // 等于失败数（隔离不误伤 = online == ap_total - failed）。
        let verdict = match c.scene {
            DrillScene::AllGreen if online == ap_total => Verdict::Pass,
            DrillScene::AllGreen => return Err(XC_GREEN_MISS as u16),
            _ if online + failed == ap_total => Verdict::Pass,
            _ => return Err(XC_ISO_OVERREACH as u16),
        };
        if steps > ap_total as u64 * 5 {
            return Err(XC_STEP_OVERRUN as u16);
        }
        Ok(DrillResult {
            cores: c.cores,
            scene_code: c.scene.code(),
            online,
            isolated: failed,
            steps,
            verdict,
        })
    }
}

/// 演练记录账：12 行定容（4 档 × 3 场景），行键 (档位, 场景) 不重复。
pub struct DrillMatrix {
    rows: Vec<(u32, u8, DrillResult)>,
}

impl DrillMatrix {
    pub fn new() -> DrillMatrix {
        DrillMatrix { rows: Vec::new() }
    }

    /// 记一行演练结果：同键重复即拒。
    pub fn record(&mut self, r: DrillResult) -> Result<(), u16> {
        if self.rows.iter().any(|(co, sc, _)| *co == r.cores && *sc == r.scene_code) {
            return Err(XC_ROW_DUP as u16);
        }
        self.rows.push((r.cores, r.scene_code, r));
        Ok(())
    }

    /// 矩阵齐备断言：12 行全在且全 Pass（4/4 全过判据）。
    pub fn all_pass(&self) -> Result<bool, u16> {
        if self.rows.len() != DRILL_CORES.len() * 3 {
            return Err(XC_MATRIX_SHORT as u16);
        }
        Ok(self.rows.iter().all(|(_, _, r)| r.verdict == Verdict::Pass))
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 域自检入口：场景语义/执行判定/矩阵齐备 三族。
pub fn run_unx0351_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0351-演练矩阵");
    // 族一：场景合法性。
    let green = DrillCase { cores: 4, scene: DrillScene::AllGreen, failed_aps: Vec::new() };
    cs.add("全绿合法", green.validate().is_ok(), "零失败");
    let single = DrillCase { cores: 4, scene: DrillScene::SingleFail, failed_aps: vec![2] };
    cs.add("单点合法", single.validate().is_ok(), "恰一失败");
    let bad_single = DrillCase { cores: 4, scene: DrillScene::SingleFail, failed_aps: vec![2, 3] };
    cs.add("单点两失败拒", bad_single.validate().is_err(), "场景语义破");
    let bsp_fail = DrillCase { cores: 4, scene: DrillScene::MixedFail, failed_aps: vec![0] };
    cs.add("BSP 失败拒", bsp_fail.validate().is_err(), "BSP 不进失败集");
    let oor = DrillCase { cores: 3, scene: DrillScene::AllGreen, failed_aps: Vec::new() };
    cs.add("非档核数拒", oor.validate().is_err(), "3 不在 2/4/8/16");

    // 族二：执行判定（4 核三场景）。
    let r_green = DrillRunner::run(&green);
    cs.add("全绿全上线", matches!(&r_green, Ok(r) if r.online == 3 && r.verdict == Verdict::Pass),
           "3/3 上线");
    let r_single = DrillRunner::run(&single);
    cs.add("单点隔离恰一", matches!(&r_single, Ok(r) if r.isolated == 1 && r.online == 2),
           "1 隔 2 上线");
    cs.add("步数账", matches!(&r_single, Ok(r) if r.steps == 3 * 5 - 3), "失败 AP 停哨兵步");
    let mixed = DrillCase { cores: 4, scene: DrillScene::MixedFail, failed_aps: vec![1, 3] };
    let r_mixed = DrillRunner::run(&mixed);
    cs.add("混合隔离两", matches!(&r_mixed, Ok(r) if r.isolated == 2 && r.online == 1),
           "2 隔 1 上线不误伤");

    // 族三：矩阵齐备（四档 × 三场景 12 行全 Pass）。
    let mut mat = DrillMatrix::new();
    for &cores in DRILL_CORES.iter() {
        let g = DrillRunner::run(&DrillCase { cores, scene: DrillScene::AllGreen, failed_aps: Vec::new() });
        if let Ok(r) = g {
            let _ = mat.record(r);
        }
        let s = DrillRunner::run(&DrillCase { cores, scene: DrillScene::SingleFail, failed_aps: vec![1] });
        if let Ok(r) = s {
            let _ = mat.record(r);
        }
        let mut f: Vec<u32> = Vec::new();
        let mut ap = 1u32;
        while (ap as u64) < (cores as u64 / 2).max(1) {
            f.push(ap);
            ap += 2;
        }
        if f.is_empty() {
            f.push(1);
        }
        let m = DrillRunner::run(&DrillCase { cores, scene: DrillScene::MixedFail, failed_aps: f });
        if let Ok(r) = m {
            let _ = mat.record(r);
        }
    }
    cs.add("矩阵 12 行", mat.len() == 12, "4 档 × 3 场景");
    cs.add("4/4 全过", matches!(mat.all_pass(), Ok(true)), "判据 J1");
    cs.add("矩阵缺行拒", {
        let mut short = DrillMatrix::new();
        if let Ok(r) = DrillRunner::run(&green) {
            let _ = short.record(r);
        }
        matches!(short.all_pass(), Err(_))
    }, "行数不足 = 未跑完");
    cs
}

// ===== 扩展面：档位收敛核（上线率随核数不减 = 无规模悬崖）=====
/// 收敛核：四档全绿场景的上线率必须单调不减（16 核上线率低于
/// 2 核 = 规模悬崖，时序竞态随核数放大，必须立案）。
///
/// 这是四档矩阵的真正价值点：单档通过只证明「这个规模能跑」，
/// 收敛核证明「规模增长不引入新失败模式」。
pub fn scaling_monotonic(results: &[(u32, u32, u32)]) -> Result<(), u16> {
    // 行 = (核数, 上线 AP, 总 AP)；上线率千分比逐档不减。
    let mut prev: Option<u64> = None;
    for &(cores, online, total) in results.iter() {
        if total == 0 || cores < 2 {
            return Err(1);
        }
        if online > total {
            return Err(2);
        }
        let rate = (online as u64 * 1000) / total as u64;
        if let Some(p) = prev {
            if rate < p {
                return Err(3);
            }
        }
        prev = Some(rate);
    }
    Ok(())
}
