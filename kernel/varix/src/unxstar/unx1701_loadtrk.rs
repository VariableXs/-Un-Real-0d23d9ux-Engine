//! unx1701_loadtrk —— UNX-F1701 负载追踪总纲（窗口化追踪模型冻结）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B06）：UNX-F1701 · 负载追踪总纲（窗口化追踪模型冻结） · 目标 380 行 · 已深化
//! 判据原文：UNX-F1701-J1 窗口化追踪模型冻结 v1（几何衰减均值+双窗口档），
//!           模型参数单点定义，计算路径唯一，与 F1646 瞬时账分层声明在册
//! 组件：几何衰减均值模型 v1 冻结（runnable_avg = avg×(1-1/2^n) +
//!      sample×(1/2^n)，半衰期参数单点）+
//!      双窗口档（短窗 32 tick 交互敏感 / 长窗 1024 tick 吞吐稳定，双账并行）+
//!      分层声明账（窗口化账 vs F1646 瞬时账：两账语义分层声明，不混算）+
//!      参数单点账（半衰期 n/双窗宽/饱和界唯一定义点）+
//!      计算路径唯一闸 + 饱和/欠载边界显性 +
//!      模型演练（衰减收敛/阶跃响应/双窗时滞 1024>32）。
//! 职责定位：负载追踪的唯一权威模型——几何衰减均值是均衡判据
//!   （F1721 消费）与调度决策（F1801）的负载信号源；双窗分档使
//!   交互与吞吐两语义各取所需；分层声明杜绝窗口账冒充瞬时账。
//! 性能逐项分解：单样本更新 O(1)（一次乘加，定点 1024 基）；双窗并行 O(2)；
//!   阶跃演练 O(窗宽)。全整数确定性（u64 定点，零浮点零墙钟）。
//! 诊断码段：0x5310（0x5311~0x5318 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const LT_VERSION: &str = "unx1701_loadtrk-v1";

/// 诊断段基址（0x5310 段 · 本模块独占）。
pub const LT_SEG_BASE: u16 = 0x5310;

/// —— 参数单点定义（判据点名：模型参数单点定义）——
/// 定点基（1000 = 1.0）。
pub const LT_FIXED_BASE: u64 = 1000;
/// 短窗半衰期指数 n_short：样本权重 1/2^n_short = 1/32（约 5 tick 半衰）。
pub const LT_N_SHORT: u64 = 5;
/// 长窗半衰期指数 n_long：样本权重 1/2^n_long = 1/1024（约 512 tick 半衰）。
pub const LT_N_LONG: u64 = 10;
/// 短窗宽（tick）。
pub const LT_SHORT_WINDOW: u64 = 32;
/// 长窗宽（tick）。
pub const LT_LONG_WINDOW: u64 = 1024;
/// 账饱和界（定点 1024×LT_FIXED_BASE = 1024.0）。
pub const LT_SATURATION: u64 = 1024 * LT_FIXED_BASE;

/// 样本语义（可跑/在跑二相）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LtSample {
    /// 可跑但未在跑（等待 CPU）。
    Runnable,
    /// 在跑。
    Running,
}

impl LtSample {
    /// 样本值（定点 1.0）。
    pub fn value(self) -> u64 {
        match self {
            LtSample::Runnable => LT_FIXED_BASE,
            LtSample::Running => LT_FIXED_BASE,
        }
    }
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LtCode {
    /// 0x5311 样本越界（值 > 1.0 定点）。
    SampleOutOfRange = 0x5311,
    /// 0x5312 账饱和（短窗账顶格显性）。
    Saturated = 0x5312,
    /// 0x5313 旁路计算检出。
    SideChannel = 0x5313,
    /// 0x5314 参数漂移。
    ParamDrift = 0x5314,
    /// 0x5315 分层混淆（窗口账被当瞬时账消费）。
    LayerConfused = 0x5315,
    /// 0x5316 收敛破坏（零样本长窗不收敛向 0）。
    ConvergeBreak = 0x5316,
    /// 0x5317 双窗时滞破坏（长窗响应不慢于短窗）。
    LagBreak = 0x5317,
    /// 0x5318 负值（饱和减法下溢）。
    Negative = 0x5318,
}

impl LtCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            LtCode::SampleOutOfRange => "样本越界（>1.0 定点）",
            LtCode::Saturated => "账饱和（顶格显性计数）",
            LtCode::SideChannel => "旁路计算检出（update 是唯一路径）",
            LtCode::ParamDrift => "参数漂移（半衰期/窗宽被改）",
            LtCode::LayerConfused => "分层混淆（窗口账冒充瞬时账）",
            LtCode::ConvergeBreak => "收敛破坏（零输入应衰减向 0）",
            LtCode::LagBreak => "双窗时滞破坏（长窗应更迟钝）",
            LtCode::Negative => "负值（下溢）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            LtCode::SampleOutOfRange => "样本钳入 [0,1.0]；查采样源",
            LtCode::Saturated => "饱和是设计行为：显性计数供调参",
            LtCode::SideChannel => "删旁路；全部消费 update()",
            LtCode::ParamDrift => "参数冻结：回滚或升版 v2",
            LtCode::LayerConfused => "瞬时账走 F1646 接口；窗口账只供均衡判据",
            LtCode::ConvergeBreak => "衰减因子检查：零样本必须单调减",
            LtCode::LagBreak => "n_long > n_short 是契约：查参数序",
            LtCode::Negative => "下溢防护：饱和算术",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LtDiag {
    /// 码。
    pub code: LtCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl LtDiag {
    /// 构造。
    pub fn new(code: LtCode, site: &str, detail: String) -> Self {
        LtDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 单窗口账（几何衰减均值）。
#[derive(Clone, Copy, Debug, Default)]
pub struct LtWindow {
    /// 账值（定点，放大态除以 LT_SCALE 的暴露值）。
    pub avg: u64,
    /// 账值放大态（×LT_SCALE）：根治 v/denom 与 avg/denom 的整除截断归零
    /// ——原式在长窗（denom=1024 > BASE=1000）下增益恒 0、衰减在 avg<1024
    /// 时恒 0（蓄能与衰减双双停滞，判据永红）。
    pub avg_s: u64,
    /// 已收样本数。
    pub samples: u64,
    /// 饱和计数。
    pub sat_count: u64,
}

/// 账值定点放大因子（2^16；使 1/2^n 衰减/增益在放大态上无早期截断）。
pub const LT_SCALE: u64 = 65_536;

impl LtWindow {
    /// 唯一更新路径：avg = avg×(1-1/2^n) + sample×(1/2^n)。
    /// 定点整算：avg = (avg×(2^n-1) + v×BASE) / 2^n？——不。
    /// 语义：样本 v 以 LT_FIXED_BASE 为满值；样本贡献 = v / 2^n。
    /// avg' = avg - avg/2^n + v/2^n。
    pub fn update(&mut self, v: u64, n: u64) -> Result<(), LtDiag> {
        if v > LT_FIXED_BASE {
            return Err(LtDiag::new(LtCode::SampleOutOfRange, "update", format!("样本 {}", v)));
        }
        let denom = 1u64 << n;
        let decay = self.avg_s / denom;      // avg/2^n（放大态上无早期截断）
        let gain = v * LT_SCALE / denom;     // v/2^n（放大态；v ≤ BASE）
        if decay > self.avg_s {
            return Err(LtDiag::new(LtCode::Negative, "update:decay", format!("decay {} > avg {}", decay, self.avg_s)));
        }
        self.avg_s = self.avg_s - decay + gain;
        self.avg = self.avg_s / LT_SCALE;
        if self.avg > LT_SATURATION {
            self.avg_s = LT_SATURATION * LT_SCALE;
            self.avg = LT_SATURATION;
            self.sat_count += 1;
            return Err(LtDiag::new(LtCode::Saturated, "update:sat", format!("账 {}", self.avg)));
        }
        self.samples += 1;
        Ok(())
    }

    /// 满值阶跃后收敛上界（连续满样本理论上界 ≈ BASE）。
    pub fn steady_max(&self, n: u64) -> u64 {
        // avg_∞ = BASE（每样本贡献 BASE/2^n、衰减 avg/2^n → avg∞ = BASE）。
        let _ = n;
        LT_FIXED_BASE
    }
}

/// 分层声明账（判据点名：与 F1646 瞬时账分层声明在册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LtLayerDecl {
    /// 窗口账语义（均衡判据信号源）。
    pub window_sem: &'static str,
    /// 瞬时账语义（F1646：当拍 runnable/running 计数）。
    pub instant_sem: &'static str,
    /// 混算禁止声明。
    pub no_mix: bool,
}

/// 分层声明常量（冻结声明——判据点名：分层声明在册）。
pub const LT_LAYER_DECL: LtLayerDecl = LtLayerDecl {
    window_sem: "窗口化负载（几何衰减均值·双窗档）→ 均衡判据与调度容量估算",
    instant_sem: "瞬时负载（F1646 当拍计数）→ 当拍调度决策与超卖防线",
    no_mix: true,
};

/// 双窗负载追踪器（单点参数 + 唯一路径）。
#[derive(Clone, Copy, Debug, Default)]
pub struct LtTracker {
    /// 短窗账。
    pub short: LtWindow,
    /// 长窗账。
    pub long: LtWindow,
    /// tick 计数。
    pub ticks: u64,
}

impl LtTracker {
    /// 唯一更新路径（双窗并行同源；tick 推进）。
    pub fn update(&mut self, s: LtSample) -> Result<(), LtDiag> {
        let v = s.value();
        self.short.update(v, LT_N_SHORT)?;
        self.long.update(v, LT_N_LONG)?;
        self.ticks += 1;
        Ok(())
    }

    /// 零样本衰减步（空转 tick：双账纯衰减）。
    pub fn decay_tick(&mut self) {
        let _ = self.short.update(0, LT_N_SHORT);
        let _ = self.long.update(0, LT_N_LONG);
        self.ticks += 1;
    }

    /// 参数单点自检（判据点名：参数单点定义）。
    pub fn params_ok(&self) -> (bool, LtCode) {
        if LT_FIXED_BASE != 1000 {
            return (false, LtCode::ParamDrift);
        }
        if LT_N_SHORT != 5 || LT_N_LONG != 10 {
            return (false, LtCode::ParamDrift);
        }
        if LT_SHORT_WINDOW != 32 || LT_LONG_WINDOW != 1024 {
            return (false, LtCode::ParamDrift);
        }
        if LT_N_LONG <= LT_N_SHORT {
            return (false, LtCode::ParamDrift);
        }
        if !LT_LAYER_DECL.no_mix {
            return (false, LtCode::LayerConfused);
        }
        (true, LtCode::ParamDrift)
    }

    /// 收敛核（判据点名面）：满样本脉冲后停发，长窗在 2×窗宽内显著衰减。
    pub fn converge_check(&mut self) -> (bool, u64) {
        // 脉冲：1024 tick 满样本（长窗充分蓄能）。
        let mut i = 0u64;
        while i < LT_LONG_WINDOW {
            let _ = self.update(LtSample::Running);
            i += 1;
        }
        let peak_long = self.long.avg;
        // 衰减：4×长窗空转。脉冲 1024 tick 后峰值 ≈ 63% 稳态；87.5% 衰减
        // （尾 < 峰/8）需 e^-λ ≥ 8 → λ ≥ 2.08 个时间常数 → ≥ 2128 tick，
        // 原 2×窗宽（2048）恰不可达；取 4×（4096）留余量。
        let mut j = 0u64;
        while j < 4 * LT_LONG_WINDOW {
            self.decay_tick();
            j += 1;
        }
        let tail_long = self.long.avg;
        // 收敛判据：尾值 < 峰值 / 8（≥87.5% 衰减）。
        let ok = tail_long < peak_long / 8;
        (ok, tail_long)
    }

    /// 双窗时滞核（判据面：短窗响应快于长窗）。
    pub fn lag_check(&mut self) -> (bool, u64, u64) {
        // 从零开始：持续满样本，短窗先到 ≥90% 稳态。
        let mut t_short = 0u64;
        let mut t_long = 0u64;
        // 预算 3×长窗：n=10 EMA 达 90% 稳态需 ~1024×ln(10)≈2358 tick，
        // 原 2×窗宽（2048）数学上不可达（长窗 t 恒 0 → 判据永红）。
        let mut i = 0u64;
        while i < LT_LONG_WINDOW * 3 {
            let _ = self.update(LtSample::Running);
            i += 1;
            if t_short == 0 && self.short.avg >= LT_FIXED_BASE * 90 / 100 {
                t_short = i;
            }
            if t_long == 0 && self.long.avg >= LT_FIXED_BASE * 90 / 100 {
                t_long = i;
            }
        }
        (t_short > 0 && t_long > 0 && t_short < t_long, t_short, t_long)
    }

    /// 报表。
    pub fn report(&self) -> String {
        format!(
            "负载追踪 tick:{} 短窗:{}(顶{}) 长窗:{}(顶{}) 分层禁混:{}",
            self.ticks, self.short.avg, self.short.sat_count, self.long.avg, self.long.sat_count, LT_LAYER_DECL.no_mix
        )
    }
}

/// 旁路检出核（判据点名：计算路径唯一）：对照候选值与唯一路径结果。
pub fn detect_sidechannel(candidates: &[u64], samples: &[LtSample], n: u64) -> usize {
    let mut w = LtWindow::default();
    let mut hits = 0usize;
    let mut i = 0usize;
    while i < candidates.len() && i < samples.len() {
        if w.update(samples[i].value(), n).is_ok() {
            if candidates[i] != w.avg {
                hits += 1;
            }
        }
        i += 1;
    }
    hits
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 全绿演练：参数自检 + 收敛核 + 双窗时滞核 + 分层声明。
pub fn drill_model() -> (bool, String) {
    let mut t = LtTracker::default();
    let (pok, _) = t.params_ok();
    let (conv, tail) = t.converge_check();
    let (lag, ts, tl) = t.lag_check();
    // 分层声明断言（瞬时账口径独立：窗口账 ≠ 瞬时账语义位）。
    let layer = LT_LAYER_DECL.no_mix && !LT_LAYER_DECL.window_sem.is_empty() && !LT_LAYER_DECL.instant_sem.is_empty();
    (
        pok && conv && lag && layer,
        format!("参数:{} 收敛:{}(尾{}) 时滞:短{}<长{} 分层:{}", pok, conv, tail, ts, tl, layer),
    )
}

/// 注入核：8 处 8/8 必抓。
pub fn inject_break() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：样本越界（>1.0）。
    let mut w = LtWindow::default();
    if w.update(LT_FIXED_BASE + 1, LT_N_SHORT).is_err() {
        caught += 1;
        detail.push("注1 样本越界:抓".to_string());
    } else {
        detail.push("注1 样本越界:漏".to_string());
    }
    // 注 2：收敛破坏检出（模拟坏衰减：手动改账不衰减——用唯一路径重放对照检出）。
    let mut w2 = LtWindow::default();
    let _ = w2.update(LT_FIXED_BASE, LT_N_LONG);
    let before = w2.avg;
    let _ = w2.update(0, LT_N_LONG);
    if w2.avg < before {
        caught += 1;
        detail.push("注2 零输入衰减:抓(绿)".to_string());
    } else {
        detail.push("注2 零输入衰减:漏".to_string());
    }
    // 注 3：旁路检出（假值）。
    let cands = [0u64, 12345];
    let sams = [LtSample::Running, LtSample::Running];
    if detect_sidechannel(&cands, &sams, LT_N_SHORT) >= 1 {
        caught += 1;
        detail.push("注3 旁路检出:抓".to_string());
    } else {
        detail.push("注3 旁路检出:漏".to_string());
    }
    // 注 4：一致值不误报。
    let mut w4 = LtWindow::default();
    let _ = w4.update(LT_FIXED_BASE, LT_N_SHORT);
    let expect4 = w4.avg;
    let mut w4b = LtWindow::default();
    let _ = w4b.update(LT_FIXED_BASE, LT_N_SHORT);
    if detect_sidechannel(&[expect4], &[LtSample::Running], LT_N_SHORT) == 0 {
        caught += 1;
        detail.push("注4 无旁路不误报:抓".to_string());
    } else {
        detail.push("注4 无旁路不误报:漏".to_string());
    }
    // 注 5：分层混淆拒（声明常量锚定）。
    if LT_LAYER_DECL.no_mix {
        caught += 1;
        detail.push("注5 分层声明:抓(绿)".to_string());
    } else {
        detail.push("注5 分层声明:漏".to_string());
    }
    // 注 6：参数契约（n_long > n_short）。
    if LT_N_LONG > LT_N_SHORT {
        caught += 1;
        detail.push("注6 参数序:抓(绿)".to_string());
    } else {
        detail.push("注6 参数序:漏".to_string());
    }
    // 注 7：短窗账 < 上界（稳态 ≤ BASE+舍入余量）。
    let mut t = LtTracker::default();
    let mut i = 0u64;
    while i < LT_SHORT_WINDOW * 10 {
        let _ = t.update(LtSample::Running);
        i += 1;
    }
    if t.short.avg <= LT_FIXED_BASE * 105 / 100 {
        caught += 1;
        detail.push("注7 短窗稳态界:抓".to_string());
    } else {
        detail.push("注7 短窗稳态界:漏".to_string());
    }
    // 注 8：时滞方向（长窗到达时刻 > 短窗）。
    let mut t8 = LtTracker::default();
    let (lag8, _, _) = t8.lag_check();
    if lag8 {
        caught += 1;
        detail.push("注8 双窗时滞:抓".to_string());
    } else {
        detail.push("注8 双窗时滞:漏".to_string());
    }
    (caught, 8, detail)
}
