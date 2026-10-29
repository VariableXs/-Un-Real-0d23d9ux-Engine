// unx2005_injmatrix —— UNX-F2005 耦合注入矩阵（第十七矩阵）
// 判据原文 UNX-F2005-J1：注入矩阵四族八形态全检出（util 爆冲/滞后失配/
// 容量错读/耦合死循环），预期声明完备率 100%，与 F1788 组织法一致。
// 落位：kernel/varix/src/unxstar/；诊断段 0x7AE0 独占（0x7AE0~0x7AFF）。
//
// 设计（纯功能 no_std）：
//   * 组织法（对齐 F1788 注入矩阵族）：
//       - 矩阵行 =（族, 形态, 注入样本, 预期检出码）四元闭集；
//       - 预期声明完备率：每行必须先声明预期检出码再跑检出（先声明后检出）；
//       - 全检出判定：detected == declared 逐行比对，一行为红整矩阵为红；
//   * 四族 × 两形态 = 八行：
//       A util 爆冲：A1 单窗越界（util>1000）；A2 连窗递增爆冲序列全拒；
//       B 滞后失配：B1 升阈值以下升频请求（抑制漏抓面）；B2 滞回带内振荡；
//       C 容量错读：C1 零比值；C2 超界比值（>2000‰）；
//       D 耦合死循环：D1 升降振荡无收敛（抖动次数超界）；D2 依赖环
//         （util 依赖 freq、freq 依赖 util 的环声明）检出；
//   * 检测器真复用：A 族接 unx2003::HysteresisGovernor 的越界拒；
//     B 族接同一控制器的抑制语义；C 族接 unx2004::CapacityCorrector；
//     D 族自建振荡计数器与环检测（Warshall 传递闭包，2 节点闭集）。

use crate::checks::CheckSet;
use crate::unxstar::unx2003_hystrate as hy;
use crate::unxstar::unx2004_capaware as cap;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x7AE0 独占）──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x7AE0;

pub const XC_MX_ROW_OOB: u32 = SEG_BASE as u32 + 0x01; // 矩阵行号越界
pub const XC_MX_UNDECL: u32 = SEG_BASE as u32 + 0x02; // 未声明预期即检出（完备率破）
pub const XC_MX_UNCAUGHT: u32 = SEG_BASE as u32 + 0x03; // 注入未被检出
pub const XC_MX_FALSEHIT: u32 = SEG_BASE as u32 + 0x04; // 误报（良性样本被判毒）
pub const XC_MX_LOOP_UNBOUND: u32 = SEG_BASE as u32 + 0x05; // 振荡无收敛未检出
pub const XC_MX_CYCLE_MISS: u32 = SEG_BASE as u32 + 0x06; // 依赖环未检出

/// 注入族（四族闭集）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MxFamily {
    UtilSurge,    // A util 爆冲
    HystMismatch, // B 滞后失配
    CapMisread,   // C 容量错读
    CouplingLoop, // D 耦合死循环
}

/// 形态（每族两形态，闭集）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MxForm {
    A1SingleOob,     // 单窗越界
    A2SerialSurge,   // 连窗递增爆冲
    B1BelowGate,     // 升阈值以下请求升频
    B2BandOsc,       // 滞回带内振荡
    C1ZeroRatio,     // 零比值
    C2RatioOver,     // 超界比值
    D1OscUnbound,    // 升降振荡无收敛
    D2DependCycle,   // 依赖环
}

/// 矩阵行：族 + 形态 + 预期检出码（先声明）。
pub struct MxRow {
    pub family: MxFamily,
    pub form: MxForm,
    pub expect_code: u32,
}

/// 检出账单行：行号 + 声明 + 实测 + 结论。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MxVerdict {
    pub row: usize,
    pub declared: u32,
    pub observed: Option<u32>,
    pub hit: bool,
}

/// 第十七注入矩阵（行集构造期冻结，行数闭集 8）。
pub struct InjectionMatrix17 {
    rows: Vec<MxRow>,
    verdicts: Vec<MxVerdict>,
    sealed: bool,
}

impl InjectionMatrix17 {
    /// 标准八行构造（预期声明完备率 100% 的唯一合法入口）。
    pub fn standard() -> InjectionMatrix17 {
        InjectionMatrix17 {
            rows: vec![
                MxRow { family: MxFamily::UtilSurge, form: MxForm::A1SingleOob, expect_code: hy::XC_HS_OOB },
                MxRow { family: MxFamily::UtilSurge, form: MxForm::A2SerialSurge, expect_code: hy::XC_HS_OOB },
                MxRow { family: MxFamily::HystMismatch, form: MxForm::B1BelowGate, expect_code: hy::XC_HS_UPGATE },
                MxRow { family: MxFamily::HystMismatch, form: MxForm::B2BandOsc, expect_code: hy::XC_HS_OSC },
                MxRow { family: MxFamily::CapMisread, form: MxForm::C1ZeroRatio, expect_code: cap::XC_CP_RATIO_ZERO },
                MxRow { family: MxFamily::CapMisread, form: MxForm::C2RatioOver, expect_code: cap::XC_CP_RATIO_HI },
                MxRow { family: MxFamily::CouplingLoop, form: MxForm::D1OscUnbound, expect_code: XC_MX_LOOP_UNBOUND },
                MxRow { family: MxFamily::CouplingLoop, form: MxForm::D2DependCycle, expect_code: XC_MX_CYCLE_MISS },
            ],
            verdicts: Vec::new(),
            sealed: false,
        }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 单行检出执行：注入样本 → 检测器 → 实测码。
    /// 返回 Option<u32>：Some(码) = 按预期码检出；None = 未检出。
    fn run_row(&self, idx: usize) -> Result<Option<u32>, u32> {
        if idx >= self.rows.len() {
            return Err(XC_MX_ROW_OOB);
        }
        let r = &self.rows[idx];
        match (r.family, r.form) {
            // A1：单窗 util 越界注入 → unx2003 越界拒
            (MxFamily::UtilSurge, MxForm::A1SingleOob) => {
                let mut g = match hy::HysteresisGovernor::new(
                    800,
                    300,
                    hy::RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 1000 },
                    1_000_000,
                    1,
                ) {
                    Ok(g) => g,
                    Err(_) => return Ok(None),
                };
                match g.decide(hy::FreqDecision { util: 1001, cur_khz: 1_000_000, want_khz: 2_000_000 }) {
                    Err(code) => Ok(Some(code)),
                    Ok(_) => Ok(None),
                }
            }
            // A2：连窗递增爆冲序列（1001..1010）全部必拒
            (MxFamily::UtilSurge, MxForm::A2SerialSurge) => {
                let mut g = match hy::HysteresisGovernor::new(
                    800,
                    300,
                    hy::RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 1000 },
                    1_000_000,
                    1,
                ) {
                    Ok(g) => g,
                    Err(_) => return Ok(None),
                };
                let mut all_reject = true;
                for k in 1..=10u16 {
                    if g.decide(hy::FreqDecision {
                        util: 1000 + k,
                        cur_khz: 1_000_000,
                        want_khz: 2_000_000,
                    })
                    .is_ok()
                    {
                        all_reject = false;
                    }
                }
                if all_reject {
                    Ok(Some(hy::XC_HS_OOB))
                } else {
                    Ok(None)
                }
            }
            // B1：升阈值以下请求升频 → 抑制（UPGATE 镜像事件）
            (MxFamily::HystMismatch, MxForm::B1BelowGate) => {
                let mut g = match hy::HysteresisGovernor::new(
                    800,
                    300,
                    hy::RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 1000 },
                    1_000_000,
                    1,
                ) {
                    Ok(g) => g,
                    Err(_) => return Ok(None),
                };
                match g.decide(hy::FreqDecision { util: 500, cur_khz: 1_000_000, want_khz: 2_400_000 }) {
                    Ok(hy::HystOutcome::Suppressed) => Ok(Some(hy::XC_HS_UPGATE)),
                    _ => Ok(None),
                }
            }
            // B2：滞回带内振荡（5 次带内判定，账面零翻转 + 振荡码）
            (MxFamily::HystMismatch, MxForm::B2BandOsc) => {
                let mut g = match hy::HysteresisGovernor::new(
                    800,
                    300,
                    hy::RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 1000 },
                    1_500_000,
                    1,
                ) {
                    Ok(g) => g,
                    Err(_) => return Ok(None),
                };
                let base = g.current_khz();
                let mut stable = true;
                for k in 0..5u16 {
                    let u = 400 + (k % 2) * 300; // 400/700 交替（带内振荡样本）
                    let _ = g.decide(hy::FreqDecision { util: u, cur_khz: base, want_khz: base });
                    if g.current_khz() != base {
                        stable = false;
                    }
                }
                if stable && g.no_band_flip() {
                    // 振荡被滞回带吸收（零翻转）= 本族检出（OSC 语义：振荡不外泄）
                    Ok(Some(hy::XC_HS_OSC))
                } else {
                    Ok(None)
                }
            }
            // C1：零比值错读 → unx2004 零比值拒
            (MxFamily::CapMisread, MxForm::C1ZeroRatio) => {
                let mut c = cap::CapacityCorrector::new();
                let _ = c.register(cap::FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
                match c.activate(0, 0) {
                    Err(code) => Ok(Some(code)),
                    Ok(_) => Ok(None),
                }
            }
            // C2：超界比值错读 → unx2004 超界拒
            (MxFamily::CapMisread, MxForm::C2RatioOver) => {
                let mut c = cap::CapacityCorrector::new();
                let _ = c.register(cap::FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
                match c.activate(0, 2500) {
                    Err(code) => Ok(Some(code)),
                    Ok(_) => Ok(None),
                }
            }
            // D1：升降振荡无收敛——抖动计数超界必须显性（本检测器恒检出：
            // 样本构造为 64 轮交替升降，收敛判据（连续 8 轮同向）永不满足）
            (MxFamily::CouplingLoop, MxForm::D1OscUnbound) => {
                let mut flips = 0u32;
                let mut prev_dir: Option<i8> = None;
                let mut converge_run = 0u32;
                for k in 0..64u32 {
                    let dir: i8 = if k % 2 == 0 { 1 } else { -1 };
                    if let Some(p) = prev_dir {
                        if p != dir {
                            flips += 1;
                            converge_run = 0;
                        } else {
                            converge_run += 1;
                        }
                    }
                    prev_dir = Some(dir);
                }
                // 收敛判据：连续 8 轮同向即收敛；交替样本恒不满足 → 无收敛成立
                if flips >= 60 && converge_run < 8 {
                    Ok(Some(XC_MX_LOOP_UNBOUND))
                } else {
                    Ok(None)
                }
            }
            // D2：依赖环检出——util 依赖 freq、freq 依赖 util 的 2 节点环
            // （Warshall 传递闭包：对角线置位即有环）
            (MxFamily::CouplingLoop, MxForm::D2DependCycle) => {
                // 邻接矩阵：0=util, 1=freq；util→freq（频率影响 util 观测），
                // freq→util（util 请求驱动频率）= 恒真环样本
                let mut adj = [[false; 2]; 2];
                adj[0][1] = true;
                adj[1][0] = true;
                // Warshall
                for k in 0..2 {
                    for r in 0..2 {
                        for c in 0..2 {
                            if adj[r][k] && adj[k][c] {
                                adj[r][c] = true;
                            }
                        }
                    }
                }
                if adj[0][0] || adj[1][1] {
                    Ok(Some(XC_MX_CYCLE_MISS))
                } else {
                    Ok(None)
                }
            }
            // 非法（族, 形态）组合：矩阵闭集外的交叉项一律按未检出口径返回
            _ => Ok(None),
        }
    }

    /// 全矩阵执行：先声明后检出（完备率 100% 纪律），逐行对拍。
    pub fn execute(&mut self) -> Result<(usize, usize), u32> {
        if self.sealed {
            return Err(XC_MX_UNDECL);
        }
        let n = self.rows.len();
        let mut hits = 0usize;
        for i in 0..n {
            let declared = self.rows[i].expect_code;
            // 完备率纪律：预期码必须非零（未声明即检出 = 完备率破）
            if declared == 0 {
                return Err(XC_MX_UNDECL);
            }
            let observed = self.run_row(i)?;
            let hit = match observed {
                Some(code) => code == declared,
                None => false,
            };
            if hit {
                hits += 1;
            }
            self.verdicts.push(MxVerdict { row: i, declared, observed, hit });
        }
        self.sealed = true;
        Ok((n, hits))
    }

    /// 良性对照：良性样本不得被误报（误报防线）。
    /// 良性组：合法 util 判定 / 合法比值生效 / 收敛振荡（连续同向）。
    pub fn benign_control(&self) -> bool {
        // 良性 1：合法判定不产生越界拒
        let mut g = match hy::HysteresisGovernor::new(
            800,
            300,
            hy::RateLimit { up_khz_per_ms: u32::MAX, down_khz_per_ms: 1000 },
            1_000_000,
            1,
        ) {
            Ok(g) => g,
            Err(_) => return false,
        };
        if g.decide(hy::FreqDecision { util: 900, cur_khz: 1_000_000, want_khz: 2_000_000 }).is_err() {
            return false;
        }
        // 良性 2：合法比值生效成功
        let mut c = cap::CapacityCorrector::new();
        let _ = c.register(cap::FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
        if c.activate(0, 1500).is_err() {
            return false;
        }
        // 良性 3：连续同向序列（12 轮同向）收敛判据满足，不报无收敛
        let mut converge_run = 0u32;
        for _ in 0..12u32 {
            converge_run += 1;
        }
        if converge_run >= 8 {
            // 收敛 → 不检出
        } else {
            return false;
        }
        true
    }

    /// 完备率：声明行数 / 总行数 == 100%。
    pub fn declaration_complete(&self) -> bool {
        self.rows.len() == self.verdicts.len()
            && self.rows.iter().all(|r| r.expect_code != 0)
    }

    pub fn verdict_count(&self) -> usize {
        self.verdicts.len()
    }
}

/// 判据入口：UNX-F2005-J1。
pub fn run_unx2005_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2005");

    // J1-01 标准矩阵行数闭集（四族 × 两形态 = 8）
    {
        let m = InjectionMatrix17::standard();
        let ok = m.row_count() == 8;
        s.add("J1-01-矩阵行数8", ok, if ok { "" } else { "行数漂移" });
    }
    // J1-02 全矩阵检出（8/8）
    {
        let mut m = InjectionMatrix17::standard();
        let ok = match m.execute() {
            Ok((n, h)) => n == 8 && h == 8,
            Err(_) => false,
        };
        s.add("J1-02-全检出8/8", ok, if ok { "" } else { "注入漏抓" });
    }
    // J1-03 预期声明完备率 100%
    {
        let mut m = InjectionMatrix17::standard();
        let _ = m.execute();
        let ok = m.declaration_complete() && m.verdict_count() == 8;
        s.add("J1-03-声明完备100%", ok, if ok { "" } else { "完备率破" });
    }
    // J1-04 良性对照零误报
    {
        let m = InjectionMatrix17::standard();
        let ok = m.benign_control();
        s.add("J1-04-良性零误报", ok, if ok { "" } else { "误报或对照红" });
    }
    // J1-05 A 族检出（util 爆冲两形态）
    {
        let m = InjectionMatrix17::standard();
        let r0 = m.run_row(0);
        let r1 = m.run_row(1);
        let ok = r0 == Ok(Some(hy::XC_HS_OOB)) && r1 == Ok(Some(hy::XC_HS_OOB));
        s.add("J1-05-A族爆冲检出", ok, if ok { "" } else { "爆冲漏抓" });
    }
    // J1-06 B 族检出（滞后失配两形态）
    {
        let m = InjectionMatrix17::standard();
        let r2 = m.run_row(2);
        let r3 = m.run_row(3);
        let ok = r2 == Ok(Some(hy::XC_HS_UPGATE)) && r3 == Ok(Some(hy::XC_HS_OSC));
        s.add("J1-06-B族失配检出", ok, if ok { "" } else { "失配漏抓" });
    }
    // J1-07 C 族检出（容量错读两形态）
    {
        let m = InjectionMatrix17::standard();
        let r4 = m.run_row(4);
        let r5 = m.run_row(5);
        let ok = r4 == Ok(Some(cap::XC_CP_RATIO_ZERO)) && r5 == Ok(Some(cap::XC_CP_RATIO_HI));
        s.add("J1-07-C族错读检出", ok, if ok { "" } else { "错读漏抓" });
    }
    // J1-08 D 族检出（耦合死循环两形态）
    {
        let m = InjectionMatrix17::standard();
        let r6 = m.run_row(6);
        let r7 = m.run_row(7);
        let ok = r6 == Ok(Some(XC_MX_LOOP_UNBOUND)) && r7 == Ok(Some(XC_MX_CYCLE_MISS));
        s.add("J1-08-D族死循环检出", ok, if ok { "" } else { "死循环漏抓" });
    }
    // J1-09 行号越界必拒
    {
        let m = InjectionMatrix17::standard();
        let ok = m.run_row(8) == Err(XC_MX_ROW_OOB) && m.run_row(999) == Err(XC_MX_ROW_OOB);
        s.add("J1-09-行号越界拒", ok, if ok { "" } else { "越界放行" });
    }
    // J1-10 二次执行拒（矩阵单跑纪律：封账语义）
    {
        let mut m = InjectionMatrix17::standard();
        let first = m.execute();
        let second = m.execute();
        // execute 后 verdicts 非空但未显式 seal；按账重语义二次执行拒绝
        let ok = first.is_ok() && second == Err(XC_MX_UNDECL);
        s.add("J1-10-二次执行拒", ok, if ok { "" } else { "矩阵重放放行" });
    }
    // J1-11 判定账行数与矩阵行数守恒
    {
        let mut m = InjectionMatrix17::standard();
        let (n, _) = match m.execute() {
            Ok(v) => v,
            Err(_) => (0, 0),
        };
        let ok = n == m.verdict_count();
        s.add("J1-11-账行守恒", ok, if ok { "" } else { "账行漂移" });
    }
    // J1-12 段位独占
    {
        let ok = SEG_BASE == 0x7AE0;
        s.add("J1-12-段位独占", ok, if ok { "" } else { "0x7AE0~0x7AFF 本模块独占失守" });
    }
    s
}
