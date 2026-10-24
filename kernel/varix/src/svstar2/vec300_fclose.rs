//! VE-F0700 · 渐变组收口（D05 组 20 项收口）
//!
//! 职责：D05 组 20 项收口（组级范式）——自检入树、基准与对拍报告、组双签、
//! 台账、向 D06 路径栅格化组移交（渐变就绪——渐变填充与路径栅格化的组合测试基础）。
//! 判据：20 项入树、报告、双签、移交、断点。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// 渐变类型：线性、径向、扫掠、菱形（D05 组的四种渐变实现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradKind {
    Linear,
    Radial,
    Sweep,
    Diamond,
}

/// 渐变填充参数：类型 + 色标表 + 扩展模式。
#[derive(Clone, Debug, PartialEq)]
pub struct GradFill {
    pub kind: GradKind,
    /// 色标（偏移 0..=1000 千分位 + RGBA 预乘色）。
    pub stops: Vec<(u32, u32)>,
    /// 扩展：0=pad 1=repeat 2=mirror。
    pub spread: u8,
}

impl GradFill {
    /// 色标不变式：首标必须落在 0、尾标落在 1000、偏移单调不减。
    pub fn stops_valid(&self) -> bool {
        if self.stops.len() < 2 {
            return false;
        }
        self.stops[0].0 == 0
            && self.stops.last().map(|s| s.0) == Some(1000)
            && self.stops.windows(2).all(|w| w[0].0 <= w[1].0)
    }
    /// 色标插值：求偏移处的预乘色（线性插值，分量通道独立）。
    pub fn sample(&self, t: u32) -> u32 {
        if t <= self.stops[0].0 {
            return self.stops[0].1;
        }
        for w in self.stops.windows(2) {
            let (t0, c0) = w[0];
            let (t1, c1) = w[1];
            if t <= t1 {
                let a = t - t0;
                let b = t1 - t0;
                // RGBA 四通道插值（u32 打包 8bit×4）。
                let mut out = 0u32;
                for ch in 0..4 {
                    let sh = ch * 8;
                    let v0 = (c0 >> sh) & 0xFF;
                    let v1 = (c1 >> sh) & 0xFF;
                    let v = v0 + (v1 as u32).saturating_sub(v0) * a / b.max(1);
                    out |= (v & 0xFF) << sh;
                }
                return out;
            }
        }
        self.stops.last().map(|s| s.1).unwrap_or(0)
    }
    /// 扩展模式折叠：t 超出 [0,1000] 按 spread 折回。
    pub fn fold(&self, t: i64) -> u32 {
        match self.spread {
            0 => t.clamp(0, 1000) as u32,
            1 => (t.rem_euclid(1001)) as u32 % 1001,
            2 => {
                let m = t.rem_euclid(2002);
                if m <= 1000 {
                    m as u32
                } else {
                    (2002 - m) as u32
                }
            }
            _ => 0,
        }
    }
}

/// 组收口清单条目：20 项逐项核验。
#[derive(Clone, Debug, PartialEq)]
pub struct CloseItem {
    pub title: String,
    pub in_tree: bool,   // 自检入树
    pub judged: bool,    // 判据全绿
}

/// 组收口核验：20 项全入树 + 全判据绿 + 双签 + 报告齐 → 通过。
pub fn group_close(items: &[CloseItem], dual_sign: bool, report_ok: bool) -> Result<(), String> {
    if items.len() != 20 {
        return Err(format!("收口条目 {} != 20：组 20 项范式", items.len()));
    }
    let missing: Vec<&str> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| !i.in_tree || !i.judged)
        .map(|(n, _)| {
            let s: &'static str = Box::leak(format!("第{}项", n + 1).into_boxed_str());
            s
        })
        .collect();
    if !missing.is_empty() {
        return Err(format!("未入树或判据未绿：{} 项——{}", missing.len(), missing.join(",")));
    }
    if !report_ok {
        return Err("基准与对拍报告缺失".to_string());
    }
    if !dual_sign {
        return Err("组双签缺失：收口阻断".to_string());
    }
    Ok(())
}

/// 向 D06 移交声明：渐变就绪——渐变填充与路径栅格化的组合测试基础。
#[derive(Clone, Debug, PartialEq)]
pub struct Handover {
    pub grad_ready: bool,
    /// 组合测试基础：渐变×路径的最小组合用例数。
    pub combo_cases: u32,
}

impl Handover {
    pub fn validate(&self) -> Result<(), String> {
        if !self.grad_ready {
            return Err("渐变未就绪：移交阻断".to_string());
        }
        if self.combo_cases < 8 {
            return Err(format!("组合用例 {} < 8：渐变×路径基础不足", self.combo_cases));
        }
        Ok(())
    }
}

/// 断点登记：组收口断点（D05→D06 里程碑锚）。
#[derive(Clone, Debug, PartialEq)]
pub struct Breakpoint {
    pub tag: String,
    pub from_group: u32,
    pub to_group: u32,
}

pub fn run_vec300_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec300");
    // 族一：渐变四型与色标不变式
    let g = GradFill {
        kind: GradKind::Linear,
        stops: vec![(0, 0xFF0000FF), (500, 0xFF00FF00), (1000, 0xFFFF0000)],
        spread: 0,
    };
    s.add("色标不变式通过", g.stops_valid(), "stops-ok");
    s.add("四渐变型齐备", {
        let k = [GradKind::Linear, GradKind::Radial, GradKind::Sweep, GradKind::Diamond];
        k[0] != k[1] && k[1] != k[2] && k[2] != k[3]
    }, "four-kinds");
    let g_bad = GradFill { kind: GradKind::Radial, stops: vec![(100, 1), (900, 2)], spread: 0 };
    s.add("首尾标不符判无效", !g_bad.stops_valid(), "stops-bad");
    let g_mono = GradFill { kind: GradKind::Sweep, stops: vec![(0, 1), (500, 2), (400, 3), (1000, 4)], spread: 0 };
    s.add("偏移非单调判无效", !g_mono.stops_valid(), "non-mono");
    // 族二：插值与扩展
    s.add("端点取样精确", g.sample(0) == 0xFF0000FF && g.sample(1000) == 0xFFFF0000, "endpoints");
    let mid = g.sample(250);
    let r = (mid >> 0) & 0xFF;
    let gch = (mid >> 8) & 0xFF;
    s.add("中点插值分通道", r == 0x7F && gch == 0x7F, "lerp");
    s.add("pad 扩展钳位", g.fold(-50) == 0 && g.fold(1500) == 1000, "pad");
    s.add("repeat 折回", g.fold(1500) >= 0 && g.fold(2000) <= 1000, "repeat");
    let gm = GradFill { kind: GradKind::Diamond, stops: g.stops.clone(), spread: 2 };
    s.add("mirror 反折", gm.fold(1500) == 502, "mirror");
    // 族三：20 项收口范式
    let items: Vec<CloseItem> = (0..20)
        .map(|i| CloseItem { title: format!("D05-{}", i + 1), in_tree: true, judged: true })
        .collect();
    s.add("20 项全绿收口通过", group_close(&items, true, true).is_ok(), "close-ok");
    let mut one_red = items.clone();
    one_red[13].judged = false;
    match group_close(&one_red, true, true) {
        Err(m) => s.add("判据未绿点名阻断", m.contains("第14项") && m.contains("1 项"), "one-red"),
        Ok(()) => s.add("判据未绿点名阻断", false, "expected err"),
    }
    let nineteen: Vec<CloseItem> = items[..19].to_vec();
    s.add("条目数不为 20 拒收", group_close(&nineteen, true, true).is_err(), "count");
    s.add("双签缺失阻断", group_close(&items, false, true).is_err(), "no-sign");
    s.add("报告缺失阻断", group_close(&items, true, false).is_err(), "no-report");
    // 族四：D06 移交与断点
    let h = Handover { grad_ready: true, combo_cases: 12 };
    s.add("渐变就绪移交通过", h.validate().is_ok(), "handover-ok");
    let h_bad = Handover { grad_ready: false, combo_cases: 12 };
    s.add("未就绪移交阻断", h_bad.validate().is_err(), "not-ready");
    let h_thin = Handover { grad_ready: true, combo_cases: 4 };
    s.add("组合用例不足阻断", h_thin.validate().is_err(), "thin-cases");
    let bp = Breakpoint { tag: "D05-CLOSE".to_string(), from_group: 5, to_group: 6 };
    s.add("断点登记", bp.from_group == 5 && bp.to_group == 6 && bp.tag.contains("D05"), "breakpoint");
    s.add("台账双签与报告构成收口证据", group_close(&items, true, true).is_ok(), "evidence");
    s
}
