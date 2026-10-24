//! veb348_mtscale —— VE-F0348 多线程伸缩验证。
//! 锚点：VE-F0348 · 目标 320 行。
//! 判据原文：梯度实测（2/4/8/16 线程）、三轮分位（每档三轮
//!           取中位分）、饱和标注（吞吐增速低于阈值实测标
//!           注）、超订对照（线程数大于核数）、诚实呈现（不
//!           线性就说不线性）、判据。
//! 组件：伸缩曲线账 + 饱和点判定 + 超订对照器。
//! 职责定位：伸缩验证是「体检报告」——几条腿能跑多快如实写；
//!           腿多了互相绊（超订）也要写；不线性就直说不线性，
//!           粉饰太平是报告造假。
//! 性能逐项分解：测量 O(梯度×轮次)；判定 O(1) 阈值；对照 O(1)。
//! 诊断码段：0x32000（0x32001~0x32009 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MT_VERSION: &str = "veb348_mtscale-v1";

/// 诊断段基址（0x32000 段 · 本模块独占）。
pub const MT_SEG_BASE: u32 = 0x32000;
pub const XC_BADTHR: u32 = 0x32001; // 线程数非法
pub const XC_BADROUND: u32 = 0x32002; // 轮次不足
pub const XC_NOSAT: u32 = 0x32003; // 无饱和点（曲线单调升未饱和）
pub const XC_FLAT: u32 = 0x32004; // 数据平坦不可判

/// 线程梯度。
pub const GRAD: [u32; 4] = [2, 4, 8, 16];
/// 轮次下限。
pub const ROUNDS_MIN: u32 = 3;
/// 饱和阈值：相邻档吞吐增速（百分比）低于此判饱和。
pub const SAT_TH: u32 = 20;

/// 一档实测（三轮吞吐）。
pub struct GradPt {
    pub threads: u32,
    /// 三轮吞吐（单位 kpx/s）。
    pub runs: [u32; 3],
}

/// 伸缩账。
pub struct ScaleBook {
    pts: Vec<GradPt>,
    /// 环境扰动登记。
    pub noise_logged: u32,
    /// 物理核数（超订对照基准）。
    pub cores: u32,
}

impl ScaleBook {
    /// 建：核数基准。
    pub fn new(cores: u32) -> ScaleBook {
        ScaleBook {
            pts: Vec::new(),
            noise_logged: 0,
            cores: cores.max(1),
        }
    }

    /// 记一档：线程非零、轮次足。
    pub fn measure(&mut self, threads: u32, runs: [u32; 3]) -> Result<(), u32> {
        if threads == 0 {
            return Err(XC_BADTHR);
        }
        let _ = ROUNDS_MIN;
        if runs.iter().any(|x| *x == 0) {
            return Err(XC_BADROUND);
        }
        self.pts.push(GradPt { threads, runs });
        Ok(())
    }

    /// 轮次波动大 → 加轮重测登记（波动 = 极差/中位 > 25%）。
    pub fn volatile(&self, threads: u32) -> Result<bool, u32> {
        let p = self
            .pts
            .iter()
            .find(|p| p.threads == threads)
            .ok_or(XC_BADTHR)?;
        let mut sorted = p.runs;
        // 三数排序。
        if sorted[0] > sorted[1] {
            sorted.swap(0, 1);
        }
        if sorted[1] > sorted[2] {
            sorted.swap(1, 2);
        }
        if sorted[0] > sorted[1] {
            sorted.swap(0, 1);
        }
        let med = sorted[1];
        if med == 0 {
            return Err(XC_FLAT);
        }
        let spread = sorted[2].saturating_sub(sorted[0]) * 100 / med;
        Ok(spread > 25)
    }

    /// 中位吞吐。
    pub fn med_of(&self, threads: u32) -> Result<u32, u32> {
        let p = self
            .pts
            .iter()
            .find(|p| p.threads == threads)
            .ok_or(XC_BADTHR)?;
        let mut sorted = p.runs;
        if sorted[0] > sorted[1] {
            sorted.swap(0, 1);
        }
        if sorted[1] > sorted[2] {
            sorted.swap(1, 2);
        }
        if sorted[0] > sorted[1] {
            sorted.swap(0, 1);
        }
        Ok(sorted[1])
    }

    /// 饱和点判定：返回首个增速 < SAT_TH 的线程档（即前一线程档为饱和点）。
    pub fn sat_point(&self) -> Result<u32, u32> {
        if self.pts.len() < 2 {
            return Err(XC_NOSAT);
        }
        let mut med = Vec::new();
        for p in self.pts.iter() {
            med.push((p.threads, self.med_of(p.threads)?));
        }
        // 按线程序排（登记序可能乱）。
        for i in 0..med.len() {
            let mut best = i;
            for j in (i + 1)..med.len() {
                if med[j].0 < med[best].0 {
                    best = j;
                }
            }
            med.swap(i, best);
        }
        for w in med.windows(2) {
            let (t0, m0) = w[0];
            let (t1, m1) = w[1];
            if m0 == 0 {
                return Err(XC_FLAT);
            }
            let gain = (m1.saturating_sub(m0)) * 100 / m0;
            if gain < SAT_TH {
                return Ok(t0);
            }
        }
        Err(XC_NOSAT)
    }

    /// 超订对照：线程档 > 核数即超订。
    pub fn oversub(&self, threads: u32) -> bool {
        threads > self.cores
    }

    /// 诚实呈现：伸缩效率（2 线程为基）；效率 <80% 显性报告「不线性」。
    pub fn linear(&self) -> Result<bool, u32> {
        let base = self.med_of(GRAD[0])?;
        if base == 0 {
            return Err(XC_FLAT);
        }
        // 看最高档效率。
        let top = self.med_of(*GRAD.last().ok_or(XC_BADTHR)?)?;
        let ideal = base * GRAD[GRAD.len() - 1] / GRAD[0];
        let eff = top * 100 / ideal.max(1);
        Ok(eff >= 80)
    }

    /// 档数。
    pub fn len(&self) -> usize {
        self.pts.len()
    }
}

pub fn run_veb348_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0348");
    // 族零：登记门。
    let mut b = ScaleBook::new(8);
    s.add("零线程拒", b.measure(0, [100, 100, 100]) == Err(XC_BADTHR), "无此档");
    s.add("零轮拒", b.measure(2, [100, 0, 100]) == Err(XC_BADROUND), "轮次缺");
    s.add("梯度 2 过", b.measure(2, [100, 102, 101]).is_ok(), "首档");
    s.add("档一", b.len() == 1, "在账");
    // 族一：三轮分位。
    s.add("中位 101", b.med_of(2) == Ok(101), "102/101/100 中位");
    s.add("查无中位拒", b.med_of(4) == Err(XC_BADTHR), "未测");
    // 波动检测。
    let _ = b.measure(4, [200, 260, 210]);
    s.add("波动大", b.volatile(4) == Ok(true), "极差/中位>25%");
    s.add("波动小", b.volatile(2) == Ok(false), "稳");
    s.add("扰动登记", {
        if b.volatile(4) == Ok(true) {
            b.noise_logged += 1;
        }
        b.noise_logged == 1
    }, "加轮重测");
    // 族二：饱和点。
    let _ = b.measure(8, [380, 390, 385]); // 4→8 增速 85% 过
    let _ = b.measure(16, [390, 392, 391]); // 8→16 增速 ~1.6% 饱和
    s.add("饱和在 8", b.sat_point() == Ok(8), "增速 <20%");
    // 单调升无饱和。
    let mut b2 = ScaleBook::new(32);
    let _ = b2.measure(2, [100, 100, 100]);
    let _ = b2.measure(4, [200, 200, 200]);
    let _ = b2.measure(8, [400, 400, 400]);
    let _ = b2.measure(16, [800, 800, 800]);
    s.add("线性无饱和", b2.sat_point() == Err(XC_NOSAT), "增速全 100%");
    // 单档无点。
    s.add("单档拒", ScaleBook::new(4).sat_point() == Err(XC_NOSAT), "曲线太短");
    // 族三：超订对照。
    s.add("核八", b.cores == 8, "基准");
    s.add("16 超订", b.oversub(16), ">8");
    s.add("8 不超", !b.oversub(8), "恰核");
    s.add("4 不超", !b.oversub(4), "欠核");
    // 族四：诚实呈现。
    // b 账 16 档 391 vs 理想 101*8=808 → 效率 48% → 不线性。
    s.add("不线性如实", b.linear() == Ok(false), "不粉饰");
    // b2 全线性。
    s.add("线性如实", b2.linear() == Ok(true), "100% 效率");
    // 族五：零核防御。
    let b3 = ScaleBook::new(0);
    s.add("零核夹一", b3.cores == 1, "防御");
    s.add("魔数核", MT_SEG_BASE == 0x32000, "段基址");
    s
}
