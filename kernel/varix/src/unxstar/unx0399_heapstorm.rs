//! unx0399_heapstorm —— UNX-F0399 堆演练矩阵（分配风暴三档）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0399 · 堆演练矩阵 · 目标 220 行
//! 判据原文：UNX-F0399-J1 堆分配风暴按三档强度各跑一整套：
//!           轻档（低频大块）、中档（均衡）、风暴档（高频小
//!           块 + 随机释放）；每档产出四指标（成功率千分/峰值
//!           存量/平均驻留/泄漏余量）；矩阵闸要求三档成功率
//!           全满 + 泄漏余量全零 + 风暴档峰值存量不超堆容量；
//!           风暴序列确定性（LCG 驱动同参同果）；收官对账：
//!           存量 = Σ分配 - Σ归还。
//! 组件：风暴档参数（强度 + 块均值）+ 风暴引擎（LCG 序列驱动的
//!       堆模型分配/释放）+ 指标采集（成功/峰值/驻留/余量）+
//!       矩阵闸（三档联合）+ 对账核。
//! 职责定位：分配风暴是「堆的压测机」——轻档验正确性，中档验
//!   均衡性，风暴档验极限：高频小块是碎片化的主要来源，随机
//!   释放让首适配算法暴露最坏行为。四指标是压测的体检报告：
//!   成功率是底线，峰值是容量依据，驻留是生命周期健康度，
//!   泄漏余量是诚实红线。
//! 性能逐项分解：单步 O(1)（模型计数器）；单档 O(步数)；矩阵闸
//!   O(1)。
//! 诊断码段：0x20E80（0x20E81~0x20E89 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HS_VERSION: &str = "unx0399_heapstorm-v1";

/// 诊断段基址（0x20E80 段 · 本模块独占）。
pub const HS_SEG_BASE: u32 = 0x20E80;
pub const XC_STORM_OOR: u32 = 0x20E81; // 档位越域
pub const XC_STEPS_OOR: u32 = 0x20E82; // 步数越域
pub const XC_CAP_OOR: u32 = 0x20E83; // 容量为零
pub const XC_SUCCESS_LO: u32 = 0x20E84; // 成功率不达标
pub const XC_LEAK: u32 = 0x20E85; // 泄漏余量非零
pub const XC_PEAK_HI: u32 = 0x20E86; // 峰值超容
pub const XC_REPLAY: u32 = 0x20E87; // 重放不一致
pub const XC_UNBAL: u32 = 0x20E88; // 收官对账破
pub const XC_STAY_NEG: u32 = 0x20E89; // 驻留负值

/// 三档强度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Storm {
    /// 轻档：低频大块。
    Light,
    /// 中档：均衡。
    Mid,
    /// 风暴档：高频小块。
    Storm,
}

impl Storm {
    /// 档位标签（1~3）。
    pub fn kind(&self) -> u8 {
        match self {
            Storm::Light => 1,
            Storm::Mid => 2,
            Storm::Storm => 3,
        }
    }

    /// 档位缺省（步数, 块均值）。
    pub fn defaults(&self) -> (u32, u32) {
        match self {
            Storm::Light => (32, 256),
            Storm::Mid => (128, 64),
            Storm::Storm => (512, 8),
        }
    }
}

/// 单档指标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StormScore {
    /// 请求总数。
    pub requests: u32,
    /// 成功数。
    pub ok: u32,
    /// 峰值存量。
    pub peak: u32,
    /// 平均驻留（成功块的步数和 / 成功数，整数近似）。
    pub avg_stay: u32,
    /// 泄漏余量（收官在册块数）。
    pub residue: u32,
}

impl StormScore {
    /// 成功率千分。
    pub fn ok_milli(&self) -> u32 {
        if self.requests == 0 {
            return 0;
        }
        self.ok * 1000 / self.requests
    }
}

/// 风暴引擎（堆容量模型：字节总量 + 在册块表）。
pub struct StormRun {
    cap: u32,
    storm: Storm,
    used: u32,
    peak: u32,
    /// 在册块：(分配步号, 字节)。
    open: Vec<(u32, u32)>,
    /// 驻留步数和。
    stay_sum: u64,
    ok: u32,
    requests: u32,
}

impl StormRun {
    /// 建：容量 + 档位。
    pub fn new(cap: u32, storm: Storm) -> Result<StormRun, u32> {
        if cap == 0 {
            return Err(XC_CAP_OOR);
        }
        if storm.kind() == 0 || storm.kind() > 3 {
            return Err(XC_STORM_OOR);
        }
        Ok(StormRun { cap, storm, used: 0, peak: 0, open: Vec::new(), stay_sum: 0, ok: 0, requests: 0 })
    }

    /// 释放最早在册块（FIFO；风暴档释放最近块制造交错）。
    fn release_one(&mut self) -> bool {
        if self.open.is_empty() {
            return false;
        }
        let idx = if self.storm == Storm::Storm { self.open.len() - 1 } else { 0 };
        let (born, bytes) = self.open.remove(idx);
        self.used -= bytes;
        self.stay_sum += (self.requests.saturating_sub(born)) as u64;
        true
    }

    /// 单步：LCG 驱动请求 k 字节（1..=kmax*2），档位控制释放频率。
    pub fn step(&mut self, tick: u32, kmax: u32) -> bool {
        let x = tick
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let span = ((x >> 16) % kmax.max(1)) + 1;
        self.requests += 1;
        // 释放频率：轻 1/8、中 1/4、风暴 1/2。
        let denom = match self.storm {
            Storm::Light => 8,
            Storm::Mid => 4,
            Storm::Storm => 2,
        };
        if self.requests % denom == 0 {
            let _ = self.release_one();
        }
        if self.used + span > self.cap {
            return false;
        }
        self.used += span;
        self.open.push((self.requests, span));
        self.ok += 1;
        if self.used > self.peak {
            self.peak = self.used;
        }
        true
    }

    /// 跑满缺省步数后收官（归还全部 + 冻指标）。
    pub fn run_and_seal(&mut self) -> Result<StormScore, u32> {
        let (steps, kavg) = self.storm.defaults();
        let kmax = kavg / 2.max(1) + 1;
        for t in 0..steps {
            self.step(t, kmax);
        }
        let residue = self.open.len() as u32;
        // 收官归还全部，累计驻留。
        while !self.open.is_empty() {
            let _ = self.release_one();
        }
        let avg = if self.ok > 0 { (self.stay_sum / self.ok as u64) as u32 } else { 0 };
        // 对账：存量 0 = Σ分 - Σ还。
        if self.used != 0 {
            return Err(XC_UNBAL);
        }
        Ok(StormScore {
            requests: self.requests,
            ok: self.ok,
            peak: self.peak,
            avg_stay: avg,
            residue,
        })
    }

    /// 容量。
    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// 当前存量。
    pub fn used(&self) -> u32 {
        self.used
    }
}

/// 单档闸：成功率满 + 泄漏零 + 峰值不超容。
pub fn gate_one(r: &StormRun, s: &StormScore) -> Result<(), u32> {
    if s.ok_milli() < 1000 {
        return Err(XC_SUCCESS_LO);
    }
    if s.residue != 0 {
        return Err(XC_LEAK);
    }
    if s.peak > r.cap() {
        return Err(XC_PEAK_HI);
    }
    Ok(())
}

/// 重放核：同参两跑指标全等。
pub fn replay_eq(cap: u32, storm: Storm) -> Result<(), u32> {
    let mut a = match StormRun::new(cap, storm) {
        Ok(x) => x,
        Err(_) => return Err(XC_CAP_OOR),
    };
    let mut b = match StormRun::new(cap, storm) {
        Ok(x) => x,
        Err(_) => return Err(XC_CAP_OOR),
    };
    let sa = a.run_and_seal()?;
    let sb = b.run_and_seal()?;
    if sa != sb {
        return Err(XC_REPLAY);
    }
    Ok(())
}

pub fn run_unx0399_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0399_heapstorm");
    // 族一：三档参数。
    cs.add("轻档参数", Storm::Light.defaults() == (32, 256), "低频大块");
    cs.add("中档参数", Storm::Mid.defaults() == (128, 64), "均衡");
    cs.add("风暴参数", Storm::Storm.defaults() == (512, 8), "高频小块");
    cs.add("档位标签", Storm::Storm.kind() == 3, "Storm=3");
    // 族二：三档全跑全绿。
    let mut r1 = match StormRun::new(1 << 20, Storm::Light) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建跑1", false, "拒");
            return cs;
        }
    };
    let s1 = r1.run_and_seal();
    cs.add("轻档跑成", s1.is_ok(), "收官对账平");
    let s1 = match s1 {
        Ok(s) => s,
        Err(_) => {
            cs.add("轻档收官", false, "破");
            return cs;
        }
    };
    cs.add("轻档成功率满", s1.ok_milli() == 1000, "1MB 充裕");
    cs.add("轻档闸过", gate_one(&r1, &s1).is_ok(), "三指标齐");
    // 中档。
    let mut r2 = match StormRun::new(1 << 20, Storm::Mid) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建跑2", false, "拒");
            return cs;
        }
    };
    let s2 = match r2.run_and_seal() {
        Ok(s) => s,
        Err(_) => {
            cs.add("中档收官", false, "破");
            return cs;
        }
    };
    cs.add("中档成功率满", s2.ok_milli() == 1000, "充裕");
    cs.add("中档闸过", gate_one(&r2, &s2).is_ok(), "三指标齐");
    // 风暴档。
    let mut r3 = match StormRun::new(1 << 20, Storm::Storm) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建跑3", false, "拒");
            return cs;
        }
    };
    let s3 = match r3.run_and_seal() {
        Ok(s) => s,
        Err(_) => {
            cs.add("风暴收官", false, "破");
            return cs;
        }
    };
    cs.add("风暴成功率满", s3.ok_milli() == 1000, "充裕");
    cs.add("风暴峰值合法", s3.peak <= 1 << 20, "不超容");
    cs.add("风暴闸过", gate_one(&r3, &s3).is_ok(), "三指标齐");
    // 族三：小容红灯（容量压到最小）。
    let mut r4 = match StormRun::new(16, Storm::Light) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建跑4", false, "拒");
            return cs;
        }
    };
    let s4 = match r4.run_and_seal() {
        Ok(s) => s,
        Err(_) => {
            cs.add("小容收官", false, "破");
            return cs;
        }
    };
    let g4 = gate_one(&r4, &s4);
    cs.add("小容峰值钳位", s4.peak <= 16, "峰值不越容");
    cs.add("小容红灯",
        g4 == Err(XC_SUCCESS_LO) || g4 == Err(XC_PEAK_HI),
        "小容必亮灯");
    // 族四：重放确定性。
    cs.add("轻档重放稳", replay_eq(1 << 20, Storm::Light).is_ok(), "全等");
    cs.add("风暴重放稳", replay_eq(1 << 20, Storm::Storm).is_ok(), "全等");
    // 族五：参数拒绝。
    cs.add("零容拒", matches!(StormRun::new(0, Storm::Light), Err(XC_CAP_OOR)), "cap=0");
    cs.add("档位标签核", Storm::Light.kind() == 1 && Storm::Mid.kind() == 2, "1/2");
    // 族六：驻留非负核。
    cs.add("驻留非负", s1.avg_stay <= s1.requests, "驻留 ≤ 总步");
    cs.add("余量零", s1.residue == 0 && s3.residue == 0, "收官清空");
    cs
}
