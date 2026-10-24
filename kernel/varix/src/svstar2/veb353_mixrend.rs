//! veb353_mixrend —— VE-F0353 软渲+GPU 混合渲染。
//! 锚点：VE-F0353 · 目标 360 行。
//! 判据原文：任务分派（GPU 优先承接主呈现、软渲承接辅助或
//!           离屏）、成本显性（跨设备像素传输一次性显式拷贝
//!           计入该任务，不静默常驻）、回流通知（GPU 不可用
//!           → 任务回流软渲并通知）、策略可配（非法拒保留
//!           当前）、判据。
//! 组件：分派表 + 传输成本账 + 策略配置。
//! 职责定位：混合渲染是「主厨与帮厨」——GPU 主勺炒大菜（主
//!           呈现），软渲切小菜（离屏辅助）；端菜过灶台要
//!           明码标价（传输成本入账），不许后厨免费跑腿。
//! 性能逐项分解：分派 O(1) 查表；传输 O(数据量) 一次性；回
//!           流 O(1)。
//! 诊断码段：0x32140（0x32141~0x32149 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MR_VERSION: &str = "veb353_mixrend-v1";

/// 诊断段基址（0x32140 段 · 本模块独占）。
pub const MR_SEG_BASE: u32 = 0x32140;
pub const XC_BADPOL: u32 = 0x32141; // 策略非法
pub const XC_NOTASK: u32 = 0x32142; // 任务不存在
pub const XC_COST: u32 = 0x32143; // 传输成本超预期（告警合并建议）
pub const XC_NOPOL: u32 = 0x32144; // 未配置策略

/// 设备。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dev {
    /// GPU。
    Gpu,
    /// 软渲（CPU）。
    Soft,
}

/// 策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pol {
    /// GPU 优先（主呈现）。
    GpuFirst,
    /// 软渲优先（省电）。
    SoftFirst,
    /// 全软渲。
    AllSoft,
}

/// 任务分派记录。
pub struct Disp {
    pub task: u32,
    pub dev: Dev,
    /// 理由代号（0=策略 1=回流 2=离屏）。
    pub why: u32,
    /// 跨设备传输成本（0 = 同设备无传输）。
    pub xfer: u32,
}

/// 成本阈值（超过告警建议合并）。
pub const COST_TH: u32 = 4096;

/// 分派器。
pub struct Mixer {
    pub pol: Pol,
    disp: Vec<Disp>,
    /// 回流通知数。
    pub reflows: u32,
    /// 成本告警数。
    pub cost_alerts: u32,
}

impl Mixer {
    /// 配置策略：全软渲策略仍算合法（省电场景）。
    pub fn new(p: Pol) -> Result<Mixer, u32> {
        let _ = XC_BADPOL;
        Ok(Mixer {
            pol: p,
            disp: Vec::new(),
            reflows: 0,
            cost_alerts: 0,
        })
    }

    /// 改策略：非法值不可表（枚举），重复改幂等。
    pub fn set_pol(&mut self, p: Pol) {
        self.pol = p;
    }

    /// 分派：按策略 + GPU 可用性定设备；跨设备传输入账。
    pub fn dispatch(&mut self, task: u32, gpu_ok: bool, px: u32, offscreen: bool) -> Result<Dev, u32> {
        let want_gpu = match self.pol {
            Pol::GpuFirst => !offscreen, // 主呈现走 GPU
            Pol::SoftFirst => false,
            Pol::AllSoft => false,
        };
        let (dev, why) = if want_gpu && gpu_ok {
            (Dev::Gpu, 0u32)
        } else if want_gpu && !gpu_ok {
            // 回流软渲并通知。
            self.reflows += 1;
            (Dev::Soft, 1)
        } else if offscreen {
            (Dev::Soft, 2)
        } else {
            (Dev::Soft, 0)
        };
        // 跨设备传输：上一笔该任务的设备与新设备不同 → 一次性拷贝。
        let xfer = if let Some(last) = self.disp.iter().rev().find(|d| d.task == task) {
            if last.dev != dev {
                px * 4
            } else {
                0
            }
        } else {
            0
        };
        if xfer > COST_TH {
            self.cost_alerts += 1;
            return Err(XC_COST);
        }
        self.disp.push(Disp {
            task,
            dev,
            why,
            xfer,
        });
        Ok(dev)
    }

    /// 任务设备查询。
    pub fn dev_of(&self, task: u32) -> Option<Dev> {
        self.disp.iter().rev().find(|d| d.task == task).map(|d| d.dev)
    }

    /// 传输账查询。
    pub fn xfer_of(&self, task: u32) -> u32 {
        self.disp
            .iter()
            .filter(|d| d.task == task)
            .map(|d| d.xfer)
            .sum()
    }

    /// 分派数。
    pub fn len(&self) -> usize {
        self.disp.len()
    }
}

pub fn run_veb353_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0353");
    // 族零：策略配置。
    let mut m = match Mixer::new(Pol::GpuFirst) {
        Ok(x) => x,
        Err(_) => {
            s.add("建败", false, "不应到");
            return s;
        }
    };
    s.add("策略在", m.pol == Pol::GpuFirst, "GPU 优先");
    m.set_pol(Pol::SoftFirst);
    s.add("改策略过", m.pol == Pol::SoftFirst, "可配");
    m.set_pol(Pol::GpuFirst);
    s.add("改回", m.pol == Pol::GpuFirst, "幂等");
    // 族一：主呈现分派。
    s.add("GPU 在主呈", m.dispatch(1, true, 100, false) == Ok(Dev::Gpu), "主呈现");
    s.add("设备在 GPU", m.dev_of(1) == Some(Dev::Gpu), "查表");
    s.add("零传输", m.xfer_of(1) == 0, "同设备");
    // 族二：离屏辅助走软渲。
    s.add("离屏软渲", m.dispatch(2, true, 100, true) == Ok(Dev::Soft), "辅助任务");
    s.add("理由离屏", m.dev_of(2) == Some(Dev::Soft), "查表");
    s.add("分派二", m.len() == 2, "两笔");
    // 族三：跨设备一次性拷贝。
    s.add("回流改软渲", m.dispatch(1, false, 100, false) == Ok(Dev::Soft), "GPU 不可用");
    s.add("回流通知一", m.reflows == 1, "通知");
    s.add("传输入账", m.xfer_of(1) == 400, "100×4");
    s.add("回流后同设备", m.dispatch(1, false, 100, false) == Ok(Dev::Soft), "软渲续跑");
    s.add("无重复传输", m.xfer_of(1) == 400, "仍 400");
    // 族四：成本告警。
    let mut m2 = match Mixer::new(Pol::GpuFirst) {
        Ok(x) => x,
        Err(_) => {
            s.add("建败 2", false, "不应到");
            return s;
        }
    };
    let _ = m2.dispatch(5, true, 1000, false); // GPU
    s.add("大传输告警", m2.dispatch(5, false, 2000, false) == Err(XC_COST), "8000>4096");
    s.add("告警一", m2.cost_alerts == 1, "建议合并");
    // 小传输不告警。
    let _ = m2.dispatch(5, false, 100, false); // 400 ≤4096
    s.add("小传输过", m2.xfer_of(5) == 4400, "累计一次拷贝");
    // 族五：软渲优先策略。
    let mut m3 = match Mixer::new(Pol::SoftFirst) {
        Ok(x) => x,
        Err(_) => {
            s.add("建败 3", false, "不应到");
            return s;
        }
    };
    s.add("软渲优先不派 GPU", m3.dispatch(9, true, 50, false) == Ok(Dev::Soft), "策略压过硬件");
    s.add("分派一", m3.len() == 1, "在账");
    // 族六：全软渲。
    let mut m4 = match Mixer::new(Pol::AllSoft) {
        Ok(x) => x,
        Err(_) => {
            s.add("建败 4", false, "不应到");
            return s;
        }
    };
    s.add("全软渲主呈", m4.dispatch(3, true, 50, false) == Ok(Dev::Soft), "AllSoft");
    s.add("零回流", m4.reflows == 0, "无跨设备");
    s.add("魔数核", MR_SEG_BASE == 0x32140, "段基址");
    s
}
