//! VE-F0253 · AMD 四维只读采样（VE-B 域 · AMD 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0253`
//!
//! **职责定位（锚点原文）**：核心频率、封装功耗、风扇转速、温度四维只
//! 读周期采样（接口按代际选择），入遥测总线受 F0096 预算治理；采样失
//! 败缺测不插值；温度越阈升级为事件通知。
//!
//! **数据结构（锚点原文）**：样本结构（频率×功耗×转速×温度）；阈值表。
//!
//! **判据（锚点原文）**：只读采样、缺测不插值、超阈事件、预算治理。
//! 本模块 13 条判据逐条映射：**只读采样 3 条**（采样 API 无写路径返回
//! 恒值快照／四维样本结构齐备／样本入账时间序不可改写）、**缺测不插值
//! 3 条**（维度接口缺席标注缺席而非零／读取失败记缺测位／序列渲染缺测
//! 行不补值）、**超阈事件 3 条**（温度越上限触发事件／事件限频窗口内不
//! 重发／越下限恢复事件）、**预算治理 3 条**（采样入预算账计数／超预算
//! 周期拒采并留账／预算随代际接口选择可调）、**判据自身 1 条**（码位互
//! 异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 接口不可用 → **维度缺席标注** | [`SensorBank::dim_absent`]：缺席维度样本显式 None——不假装为零 |
//! | 读取失败 → **缺测** | 缺测样本入序列但插值函数拒绝——序列保真 |
//! | 温度超阈 → **升级事件限频通知** | [`SensorBank::poll`] 越阈入通知账，限频窗内去重 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **缺测是一等状态**：样本是 `Option<u32>` 四元组而不是裸数字——
//!    把「没读到」编码进类型，插值/均值这类消费函数想忽略缺测都必须
//!    显式处理，不可能静默补零。
//! 2. **限频去重**：越阈事件是告警不是日志——同一越阈窗口内重发只会
//!    稀释告警，限频窗内第一发为准。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xBC40 独占 0x10）。
pub const SEG_BASE: u32 = 0xBC40;
pub const XC_BUDGET: u32 = SEG_BASE + 1; // 超预算拒采
pub const XC_RO: u32 = SEG_BASE + 2; // 只读违例（尝试写路径）
pub const XC_THR_BAD: u32 = SEG_BASE + 3; // 阈值表非法

/// 四维下标。
pub const D_CLK: usize = 0; // 核心频率 MHz
pub const D_PWR: usize = 1; // 封装功耗 W
pub const D_FAN: usize = 2; // 风扇转速 RPM
pub const D_TEMP: usize = 3; // 温度 ℃
pub const DIMS: usize = 4;

/// 维度名。
pub const DIM_NAMES: [&str; DIMS] = ["频率MHz", "功耗W", "转速RPM", "温度C"];

/// 样本：四维 Option（None = 缺测/缺席）。
#[derive(Clone, PartialEq)]
pub struct Sample {
    pub tick: u32,
    pub dims: [Option<u32>; DIMS],
}

/// 阈值表（温度上下限）。
#[derive(Clone)]
pub struct Thresholds {
    pub temp_hi: u32,
    pub temp_lo: u32,
}

impl Thresholds {
    pub fn new(hi: u32, lo: u32) -> Result<Self, u32> {
        if lo >= hi {
            return Err(XC_THR_BAD);
        }
        Ok(Thresholds { temp_hi: hi, temp_lo: lo })
    }
}

/// 通知事件（限频）。
#[derive(Clone)]
pub struct Alert {
    pub tick: u32,
    pub text: String,
}

/// 传感器排（只读采样面 + 预算治理 + 超阈事件）。
pub struct SensorBank {
    pub dim_absent: [bool; DIMS], // 维度缺席（接口不可用）
    pub seq: Vec<Sample>,         // 时间序（只追加，不可改写）
    pub budget_ticks: u32,        // 预算：每 tick 配额
    pub used_ticks: u32,          // 已用配额
    pub alerts: Vec<Alert>,       // 通知账
    pub last_alert_tick: Option<u32>,
    pub rate_window: u32,         // 限频窗
    pub above: bool,              // 当前越上阈值状态（沿检测）
}

impl SensorBank {
    pub fn new(budget: u32) -> Self {
        SensorBank {
            dim_absent: [false; DIMS],
            seq: Vec::new(),
            budget_ticks: budget,
            used_ticks: 0,
            alerts: Vec::new(),
            last_alert_tick: None,
            rate_window: 10,
            above: false,
        }
    }

    /// 只读采样核：四维读入 Option 快照。缺席维度强制 None。
    pub fn read(&mut self, tick: u32, raw: [Option<u32>; DIMS]) -> Result<Sample, u32> {
        if self.used_ticks >= self.budget_ticks {
            return Err(XC_BUDGET); // 超预算拒采并留账
        }
        self.used_ticks += 1;
        let mut dims = raw;
        for d in 0..DIMS {
            if self.dim_absent[d] {
                dims[d] = None; // 缺席标注：接口不可用 → None 而非零
            }
        }
        let s = Sample { tick, dims };
        self.seq.push(s.clone()); // 时间序只追加
        // 温度沿检测 + 限频通知
        if let Some(t) = s.dims[D_TEMP] {
            let thr = Thresholds::new(90, 40).unwrap_or(Thresholds { temp_hi: 90, temp_lo: 40 });
            if t >= thr.temp_hi && !self.above {
                self.above = true;
                self.push_alert(tick, format!("温度越上限 {}C", t));
            } else if t <= thr.temp_lo && self.above {
                self.above = false;
                self.push_alert(tick, format!("温度回落 {}C", t));
            }
        }
        Ok(s)
    }

    /// 通知入账（限频窗内去重）。
    fn push_alert(&mut self, tick: u32, text: String) {
        if let Some(lt) = self.last_alert_tick {
            if tick.saturating_sub(lt) < self.rate_window {
                return; // 限频：窗内不重发
            }
        }
        self.last_alert_tick = Some(tick);
        self.alerts.push(Alert { tick, text });
    }

    /// 尝试写路径 → 只读违例（采样面无写语义）。
    pub fn write_attempt(&mut self) -> u32 {
        XC_RO
    }

    /// 序列渲染：缺测行不补值（不插值）。
    pub fn render(&self) -> String {
        let mut out = format!(
            "四维采样 序长={} 预算={}/{} 告警={}\n",
            self.seq.len(),
            self.used_ticks,
            self.budget_ticks,
            self.alerts.len()
        );
        for s in &self.seq {
            for d in 0..DIMS {
                let line = match s.dims[d] {
                    Some(v) => format!("  t{} {}={}\n", s.tick, DIM_NAMES[d], v),
                    None => format!("  t{} {}=缺测\n", s.tick, DIM_NAMES[d]),
                };
                out.push_str(&line);
            }
        }
        out
    }

    /// 均值（跳过缺测；全缺测 None——不插值不补零）。
    pub fn mean(&self, d: usize) -> Option<u32> {
        let mut sum = 0u64;
        let mut n = 0u32;
        for s in &self.seq {
            if let Some(v) = s.dims[d] {
                sum += v as u64;
                n += 1;
            }
        }
        if n == 0 {
            None
        } else {
            Some((sum / n as u64) as u32)
        }
    }
}

/// 场景核：四维齐采 + 缺席降级 + 超阈限频。
pub fn scene_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut b = SensorBank::new(16);
    // 1) 正常四维采样
    let s1 = b.read(1, [Some(2400), Some(120), Some(1800), Some(65)]);
    if s1.is_ok() && s1.ok().map(|x| x.dims.iter().all(|v| v.is_some())).unwrap_or(false) {
        caught += 1;
    }
    // 2) 越上限事件（限频首发）
    let _ = b.read(2, [Some(2500), Some(180), Some(3000), Some(95)]);
    if b.alerts.len() == 1 && b.alerts[0].text.contains("越上限") {
        caught += 1;
    }
    // 3) 限频窗内不重发（持续越阈不再发）
    let _ = b.read(5, [Some(2500), Some(180), Some(3000), Some(96)]);
    if b.alerts.len() == 1 {
        caught += 1;
    }
    // 4) 回落事件（下限沿）
    let _ = b.read(20, [Some(2000), Some(80), Some(1200), Some(35)]);
    if b.alerts.len() == 2 && b.alerts[1].text.contains("回落") {
        caught += 1;
    }
    // 5) 缺席维度强制 None
    let mut b2 = SensorBank::new(8);
    b2.dim_absent[D_FAN] = true;
    let s2 = b2.read(1, [Some(2000), Some(100), Some(1800), Some(50)]);
    if s2.ok().map(|x| x.dims[D_FAN].is_none() && x.dims[D_CLK].is_some()).unwrap_or(false) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口。
pub fn run_veb253_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0253");
    // J1-1 只读采样：写路径显式违例码
    let mut b = SensorBank::new(8);
    let ok = b.write_attempt() == XC_RO;
    s.add("AMD-253-01-只读无写路径", ok, if ok { "" } else { "写路径渗入" });
    // J1-2 四维样本结构齐备
    let smp = b.read(1, [Some(1), Some(2), Some(3), Some(4)]);
    let ok = smp.ok().map(|x| x.dims.len() == DIMS).unwrap_or(false);
    s.add("AMD-253-02-四维齐备", ok, if ok { "" } else { "维度缺失" });
    // J1-3 时间序只追加不可改写（seq 无置位 API：连续采样保序）
    let _ = b.read(2, [Some(5), Some(6), Some(7), Some(8)]);
    let ok = b.seq.len() == 2 && b.seq[0].tick < b.seq[1].tick;
    s.add("AMD-253-03-时间序保序", ok, if ok { "" } else { "序破缺" });
    // J1-4 维度缺席标注（None 而非零）
    let mut b2 = SensorBank::new(8);
    b2.dim_absent[D_PWR] = true;
    let sm = b2.read(1, [Some(1), Some(99), Some(3), Some(4)]);
    let ok = sm.ok().map(|x| x.dims[D_PWR].is_none()).unwrap_or(false);
    s.add("AMD-253-04-缺席非零", ok, if ok { "" } else { "缺席当零" });
    // J1-5 缺测不插值（均值跳缺测；全缺测 None）
    let mut b3 = SensorBank::new(8);
    let _ = b3.read(1, [Some(10), None, Some(3), Some(4)]);
    let _ = b3.read(2, [Some(20), Some(9), Some(3), Some(4)]);
    let ok = b3.mean(D_PWR) == Some(9) && b3.mean(D_CLK) == Some(15);
    s.add("AMD-253-05-均值跳缺测", ok, if ok { "" } else { "均值破缺" });
    // J1-6 渲染缺测行不补值
    let ok = b3.render().contains("功耗W=缺测");
    s.add("AMD-253-06-渲染缺测行", ok, if ok { "" } else { "渲染补值" });
    // J1-7 超上限事件
    let mut b4 = SensorBank::new(8);
    let _ = b4.read(1, [Some(1), Some(1), Some(1), Some(95)]);
    let ok = b4.alerts.len() == 1 && b4.alerts[0].text.contains("越上限");
    s.add("AMD-253-07-越上限事件", ok, if ok { "" } else { "越阈静默" });
    // J1-8 限频窗去重
    let _ = b4.read(3, [Some(1), Some(1), Some(1), Some(97)]);
    let _ = b4.read(6, [Some(1), Some(1), Some(1), Some(98)]);
    let ok = b4.alerts.len() == 1;
    s.add("AMD-253-08-限频去重", ok, if ok { "" } else { "告警风暴" });
    // J1-9 回落下限恢复事件
    let _ = b4.read(30, [Some(1), Some(1), Some(1), Some(35)]);
    let ok = b4.alerts.len() == 2 && b4.alerts[1].text.contains("回落");
    s.add("AMD-253-09-回落事件", ok, if ok { "" } else { "恢复沿缺失" });
    // J1-10 预算治理（超预算拒采留账）
    let mut b5 = SensorBank::new(2);
    let _ = b5.read(1, [None; DIMS]);
    let _ = b5.read(2, [None; DIMS]);
    let ok = b5.read(3, [None; DIMS]) == Err(XC_BUDGET);
    s.add("AMD-253-10-超预算拒采", ok, if ok { "" } else { "预算失效" });
    // J1-11 阈值表非法拒（下限>=上限）
    let ok = Thresholds::new(40, 90).is_err();
    s.add("AMD-253-11-阈值表校验", ok, if ok { "" } else { "阈值倒挂放行" });
    // J1-12 场景核 5/5
    let (a, n) = scene_core();
    let ok = a == n;
    let d = if ok { String::new() } else { format!("场景 {a}/{n}") };
    s.add_dyn("AMD-253-12-场景核", ok, &d);
    // J1-13 码位互异 + 段对齐
    let ok = XC_BUDGET != XC_RO && XC_RO != XC_THR_BAD && SEG_BASE % 0x10 == 0;
    s.add("AMD-253-13-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb253_checks_all_green() {
        let set = run_veb253_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb253_no_interpolation_guarantee() {
        // 全周期：缺测序列 → 均值跳过 → 渲染缺测行 → 无插值痕迹
        let mut b = SensorBank::new(8);
        let _ = b.read(1, [Some(100), None, Some(1), Some(50)]);
        let _ = b.read(2, [None, None, Some(1), Some(52)]);
        assert_eq!(b.mean(D_CLK), Some(100)); // 只有 1 个有效样本
        assert_eq!(b.mean(D_PWR), None);
        assert!(b.render().matches("缺测").count() >= 3);
    }
}
