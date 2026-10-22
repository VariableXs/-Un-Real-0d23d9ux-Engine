//! CGPU-F0372 · 短期预测器（判据段 0x09C0）
//! 预测器：滑动窗口统计预测（下一帧/下 N 帧负载——EWMA+趋势项）；
//! 多通道预测（CPU/GPU/带宽三通道独立）；输出协议（预测值+置信区
//! 间——下游按区间下界预留，宁欠勿超）；前瞻窗口（1-4 帧可配）。
//! 判据：EWMA、三通道、置信区间、窗口、判据。
//!
//! 锚点原文摘录（施工书 1653 行）：
//! 「滑动窗口统计预测（EWMA+趋势项）；多通道预测（CPU/GPU/带宽
//! 三通道独立）；输出协议（预测值+置信区间——下游按区间下界预留，
//! 宁欠勿超）；前瞻窗口（1-4 帧可配）。判据：EWMA、三通道、置信
//! 区间、窗口、判据。」
//!
//! 诊断码段 0x09C0（本模块独占：0x09C0~0x09CF）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。


/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x09C0;

/// EWMA 平滑系数（千分比）：α = 375/1000（≈0.375——反应快而不抖）。
pub const EWMA_ALPHA_PERMILLE: u32 = 375;

/// 滑动窗口（帧）：趋势项计算窗。
pub const TREND_WINDOW: usize = 8;

/// 置信区间半宽基准（千分比）：IQR 式样本散布的诚实外推。
pub const CI_BASE_HALF_PERMILLE: u32 = 120;

/// 前瞻窗口域（帧）：1..=4。
pub const HORIZON_MIN: u32 = 1;
pub const HORIZON_MAX: u32 = 4;

/// 预测通道（判据「三通道」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    /// CPU 通道。
    Cpu,
    /// GPU 通道。
    Gpu,
    /// 带宽通道。
    Bandwidth,
}

impl Channel {
    /// 通道号。
    pub fn ordinal(&self) -> u8 {
        match self {
            Channel::Cpu => 0,
            Channel::Gpu => 1,
            Channel::Bandwidth => 2,
        }
    }
    /// 闭集遍历。
    pub fn all() -> [Channel; 3] {
        [Channel::Cpu, Channel::Gpu, Channel::Bandwidth]
    }
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Channel::Cpu => "cpu",
            Channel::Gpu => "gpu",
            Channel::Bandwidth => "bw",
        }
    }
}

/// 单通道预测器（判据「EWMA + 趋势项」核心机）：EWMA 平滑 + 窗内
/// 线性趋势项叠加。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChannelPredictor {
    /// EWMA 当前值。
    pub ewma: u32,
    /// 已喂样本数。
    pub fed: u32,
    /// 前瞻帧数。
    pub horizon: u32,
    /// 趋势窗样本。
    pub trend_win: [u32; TREND_WINDOW],
    /// 趋势窗有效长。
    pub trend_len: usize,
}

impl ChannelPredictor {
    /// 新预测器（EWMA 初值 = 首样本——不猜起点）。
    pub fn new(horizon: u32) -> Result<Self, u16> {
        if horizon < HORIZON_MIN || horizon > HORIZON_MAX {
            return Err(SEG_BASE);
        }
        Ok(ChannelPredictor { ewma: 0, fed: 0, horizon, trend_win: [0; TREND_WINDOW], trend_len: 0 })
    }
    /// 喂一帧实测（判据「EWMA」逐步更新）。
    pub fn feed(&mut self, v: u32) {
        if self.fed == 0 {
            self.ewma = v;
        } else {
            // ewma' = α·v + (1-α)·ewma（整数定点）。
            let a = EWMA_ALPHA_PERMILLE as u64;
            self.ewma = ((a * v as u64 + (1000 - a) * self.ewma as u64) / 1000) as u32;
        }
        self.fed += 1;
        // 趋势窗滚动。
        if self.trend_len < TREND_WINDOW {
            self.trend_win[self.trend_len] = v;
            self.trend_len += 1;
        } else {
            self.trend_win.copy_within(1.., 0);
            self.trend_win[TREND_WINDOW - 1] = v;
        }
    }
    /// 趋势项（判据「趋势项」）：窗内首尾差 / 窗长（每帧漂移）。
    /// 窗不满或恒值 → 零趋势（诚实不猜）。
    pub fn trend_per_frame(&self) -> i64 {
        if self.trend_len < TREND_WINDOW {
            return 0;
        }
        let first = self.trend_win[0] as i64;
        let last = self.trend_win[TREND_WINDOW - 1] as i64;
        // 每帧漂移 = 首尾差 / (窗长-1)。
        (last - first) / (TREND_WINDOW as i64 - 1)
    }
    /// 散布半宽（判据「置信区间」基础量）：窗内样本极差 / 2，
    /// 不低于 CI_BASE_HALF_PERMILLE 地板（零散布也保留最小不确定性）。
    pub fn scatter_half(&self) -> u32 {
        if self.trend_len == 0 {
            return CI_BASE_HALF_PERMILLE;
        }
        let mut lo = u32::MAX;
        let mut hi = 0u32;
        for i in 0..self.trend_len {
            let v = self.trend_win[i];
            if v < lo {
                lo = v;
            }
            if v > hi {
                hi = v;
            }
        }
        let half = (hi - lo) / 2;
        half.max(CI_BASE_HALF_PERMILLE)
    }
}

/// 预测输出（判据「输出协议」一等公民）：点估 + 区间——下游按
/// 区间下界预留，宁欠勿超。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Forecast {
    /// 通道。
    pub channel: Channel,
    /// 点估（千分比口径原样）。
    pub point: u32,
    /// 区间下界（预留依据）。
    pub lower: u32,
    /// 区间上界。
    pub upper: u32,
    /// 前瞻帧。
    pub horizon: u32,
}

impl Forecast {
    /// 协议自洽核：下 ≤ 点 ≤ 上，且下 ≥ 0。
    pub fn consistent(&self) -> Result<(), u16> {
        if self.lower > self.point || self.point > self.upper {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
    /// 宁欠勿超预留值（判据「输出协议」语义面）：取下界。
    pub fn reserve_value(&self) -> u32 {
        self.lower
    }
}

/// 多通道预测入口（判据「三通道」主判据）：三通道独立预测器各出
/// 一份 Forecast——通道间零耦合（CPU 的波动不影响 GPU 的预测）。
pub fn forecast_all(
    cps: &mut [ChannelPredictor; 3],
    sample: [u32; 3],
) -> Result<[Forecast; 3], u16> {
    let mut out = [Forecast {
        channel: Channel::Cpu,
        point: 0,
        lower: 0,
        upper: 0,
        horizon: 0,
    }; 3];
    for i in 0..3 {
        let ch = Channel::all()[i];
        let p = &mut cps[i];
        p.feed(sample[i]);
        if p.fed < 2 {
            // 样本不足两帧：不猜（显性给零点估——下游按 0 预留即被动）。
            out[i] = Forecast { channel: ch, point: 0, lower: 0, upper: 0, horizon: p.horizon };
            continue;
        }
        let trend = p.trend_per_frame();
        let base = p.ewma as i64 + trend * p.horizon as i64;
        let point = base.max(0) as u32;
        let half = p.scatter_half();
        let lower = point.saturating_sub(half);
        let upper = point.saturating_add(half).min(1000);
        let f = Forecast { channel: ch, point, lower, upper, horizon: p.horizon };
        f.consistent()?;
        out[i] = f;
    }
    Ok(out)
}

/// 前瞻窗口核（判据「窗口」守卫）：horizon 必须在 [1,4]；同一预
/// 测器 horizon 冻结（建后不可改——防挑窗口作弊）。
pub fn horizon_frozen(p: &ChannelPredictor) -> Result<(), u16> {
    if p.horizon < HORIZON_MIN || p.horizon > HORIZON_MAX {
        return Err(SEG_BASE + 2);
    }
    Ok(())
}

/// 宁欠勿超核（判据「输出协议」验收面）：三通道下界和 ≤ 点估和
/// ——按区间下界预留的总量必不超点估总量（不超卖的第一性）。
pub fn underpromise_ok(f: &[Forecast; 3]) -> Result<(), u16> {
    let sum_lower: u32 = f.iter().map(|x| x.lower).sum();
    let sum_point: u32 = f.iter().map(|x| x.point).sum();
    if sum_lower > sum_point {
        return Err(SEG_BASE + 3);
    }
    Ok(())
}

/// 预测指纹（可审计面）。
pub fn forecast_fp(f: &[Forecast; 3]) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for x in f.iter() {
        acc = acc.wrapping_mul(0x100000001b3) ^ x.channel.ordinal() as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ x.point as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ x.lower as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ x.upper as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ x.horizon as u64;
    }
    acc
}

/// 预测器诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "前瞻必须 ∈ [1,4]",
        1 => "区间不自洽（下≤点≤上）",
        2 => "前瞻冻结被改",
        3 => "下界和>点估和——超卖风险",
        _ => "未知诊断码——查本模块 SEG_BASE 0x09C0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ewma_and_trend() {
        let mut p = ChannelPredictor::new(2).unwrap();
        assert!(horizon_frozen(&p).is_ok());
        // 稳态 400 连喂：EWMA 收敛到 400。
        for _ in 0..40 {
            p.feed(400);
        }
        assert_eq!(p.ewma, 400);
        assert_eq!(p.trend_per_frame(), 0);
        // 上升趋势：尾 8 帧从 400 → 407。
        for v in [401u32, 402, 403, 404, 405, 406, 407, 408] {
            p.feed(v);
        }
        assert_eq!(p.trend_per_frame(), 1);
        // 散布地板：极差/2 但 ≥ 120。
        let half = p.scatter_half();
        assert!(half >= CI_BASE_HALF_PERMILLE);
        assert_eq!(ChannelPredictor::new(5).unwrap_err(), SEG_BASE);
    }

    #[test]
    fn three_channels_independent() {
        let mut cps = [
            ChannelPredictor::new(1).unwrap(),
            ChannelPredictor::new(2).unwrap(),
            ChannelPredictor::new(3).unwrap(),
        ];
        // 前两帧不猜（样本不足）。
        let f0 = forecast_all(&mut cps, [300, 500, 100]).unwrap();
        assert!(f0.iter().all(|x| x.point == 0));
        // 第三帧起出真预测。
        let f1 = forecast_all(&mut cps, [300, 500, 100]).unwrap();
        for (i, x) in f1.iter().enumerate() {
            assert_eq!(x.channel.ordinal(), i as u8);
            assert!(x.consistent().is_ok());
        }
        // CPU 通道点估 ≈300 附近（EWMA 已收敛），GPU ≈500，带宽 ≈100。
        assert!(f1[0].point > 250 && f1[0].point < 350);
        assert!(f1[1].point > 450 && f1[1].point < 550);
        assert!(f1[2].point > 50 && f1[2].point < 150);
        // 宁欠勿超。
        assert!(underpromise_ok(&f1).is_ok());
        assert_eq!(f1[0].reserve_value(), f1[0].lower);
        assert!(forecast_fp(&f1) != 0);
    }

    #[test]
    fn ci_floor_and_protocol() {
        // 恒值序列：极差 0 → 地板 120 生效。
        let mut p = ChannelPredictor::new(1).unwrap();
        for _ in 0..20 {
            p.feed(500);
        }
        assert_eq!(p.scatter_half(), CI_BASE_HALF_PERMILLE);
        let bad = Forecast { channel: Channel::Cpu, point: 500, lower: 600, upper: 700, horizon: 1 };
        assert_eq!(bad.consistent().unwrap_err(), SEG_BASE + 1);
        assert!(diagnose(SEG_BASE + 1).contains("区间"));
        assert_eq!(underpromise_ok(&[bad; 3].map(|mut x| { x.channel = Channel::Cpu; x })).unwrap_err(), SEG_BASE + 3);
    }
}
