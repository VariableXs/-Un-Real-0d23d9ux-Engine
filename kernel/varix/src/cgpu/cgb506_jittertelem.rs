//! CGPU-F0506 · 抖动遥测（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 遥测：抖动谱/均衡器策略命中/抑制率/残余抖动/缓冲深度分布——五指标
//!   进 B08 总线；按呈现模式分列。判据：五指标、分列、判据。
//!
//! 设计纪律：
//! - 五指标结构：① 抖动谱（8 桶：0-1/1-2/2-4/4-8/8-16/16-32/32-64/64+
//!   ms 的间隔抖动幅度分布）；② 策略命中（四策略各命中次数：直通/
//!   深缓冲/提前渲染/组合）；③ 抑制率（抑制事件 ÷ 检测事件，‰ 计）；
//!   ④ 残余抖动（抑制后残余幅度累计与计数——均值口径）；⑤ 缓冲深度
//!   分布（0-7 级桶）。
//! - 呈现模式分列（F0499 三模式）：垂直同步/邮箱/立即——查询必须带
//!   模式码，无模式的聚合值不提供（分列是结构性的）。
//! - B08 帧：JT_MAGIC=0xB08F_0506，五指标打包 12 word + 校验字。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x5210 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x5210，批十段首，全仓独占）。
pub const SEG_BASE: u16 = 0x5210;

pub const E_JT_MODE: u16 = SEG_BASE + 1; // 呈现模式非法
pub const E_JT_STRAT: u16 = SEG_BASE + 2; // 策略码非法
pub const E_JT_DEPTH: u16 = SEG_BASE + 3; // 缓冲深度越界
pub const E_JT_EMPTY: u16 = SEG_BASE + 4; // 空列无指标
pub const E_JT_MAGIC: u16 = SEG_BASE + 5; // B08 校验失败

/// 呈现模式码（F0499 三模式）。
pub const PM_VSYNC: u8 = 1;
pub const PM_MAILBOX: u8 = 2;
pub const PM_IMMEDIATE: u8 = 3;
pub const MODES: [u8; 3] = [PM_VSYNC, PM_MAILBOX, PM_IMMEDIATE];

/// 均衡器策略码（D02 五源分类学的处置面）。
pub const ST_PASSTHRU: u8 = 0; // 直通（不干预）
pub const ST_DEEPBUF: u8 = 1; // 深缓冲吸收
pub const ST_EARLYREN: u8 = 2; // 提前渲染窗口
pub const ST_COMBO: u8 = 3; // 组合策略
pub const STRATS: [u8; 4] = [ST_PASSTHRU, ST_DEEPBUF, ST_EARLYREN, ST_COMBO];

/// 谱桶上界（us）：8 桶，末桶为 64ms+ 开区间。
pub const SPEC_EDGES_US: [u64; 7] = [
    1_000, 2_000, 4_000, 8_000, 16_000, 32_000, 64_000,
];

/// 缓冲深度合法域 0..=7。
pub const BUF_DEPTHS: u8 = 8;

/// B08 帧魔数与打包字数。
pub const JT_MAGIC: u32 = 0xB08F_0506;
pub const JT_WORDS: usize = 12;

/// 单模式五指标列。
#[derive(Clone, Debug)]
pub struct JitterCol {
    /// 抖动谱 8 桶计数。
    pub spec: [u64; 8],
    /// 策略命中 4 计数。
    pub strat_hits: [u64; 4],
    /// 检测到的抖动事件数。
    pub detected: u64,
    /// 其中被抑制的事件数。
    pub suppressed: u64,
    /// 残余抖动累计（us，抑制后实测）。
    pub resid_us: u64,
    /// 残余抖动样本数。
    pub resid_n: u64,
    /// 缓冲深度分布 8 桶。
    pub buf_hist: [u64; 8],
}

impl JitterCol {
    fn new() -> Self {
        Self {
            spec: [0; 8],
            strat_hits: [0; 4],
            detected: 0,
            suppressed: 0,
            resid_us: 0,
            resid_n: 0,
            buf_hist: [0; 8],
        }
    }

    /// 抑制率（‰）；无检测事件时返回 0。
    pub fn supp_rate_pm(&self) -> u64 {
        if self.detected == 0 {
            0
        } else {
            self.suppressed * 1_000 / self.detected
        }
    }

    /// 残余抖动均值（us）；无样本返回 0。
    pub fn resid_mean_us(&self) -> u64 {
        if self.resid_n == 0 {
            0
        } else {
            self.resid_us / self.resid_n
        }
    }

    /// 谱主桶（计数最大的桶号；空列返回 0）。
    pub fn dominant_bucket(&self) -> usize {
        let mut best = 0usize;
        for i in 1..8 {
            if self.spec[i] > self.spec[best] {
                best = i;
            }
        }
        best
    }
}

/// 抖动遥测账本：三模式分列。
#[derive(Clone, Debug)]
pub struct JitterTelem {
    cols: Vec<JitterCol>,
}

impl JitterTelem {
    pub fn new() -> Self {
        Self {
            cols: vec![JitterCol::new(), JitterCol::new(), JitterCol::new()],
        }
    }

    fn idx(mode: u8) -> Result<usize, u16> {
        match mode {
            PM_VSYNC => Ok(0),
            PM_MAILBOX => Ok(1),
            PM_IMMEDIATE => Ok(2),
            _ => Err(E_JT_MODE),
        }
    }

    fn spec_bucket(jitter_us: u64) -> usize {
        for (i, e) in SPEC_EDGES_US.iter().enumerate() {
            if jitter_us < *e {
                return i;
            }
        }
        7
    }

    /// 记录一次抖动事件：幅度/命中策略/是否被抑制/当帧缓冲深度。
    pub fn record(
        &mut self,
        mode: u8,
        jitter_us: u64,
        strategy: u8,
        suppressed: bool,
        buf_depth: u8,
    ) -> Result<(), u16> {
        let i = Self::idx(mode)?;
        if !STRATS.contains(&strategy) {
            return Err(E_JT_STRAT);
        }
        if buf_depth >= BUF_DEPTHS {
            return Err(E_JT_DEPTH);
        }
        let c = &mut self.cols[i];
        c.spec[Self::spec_bucket(jitter_us)] += 1;
        c.strat_hits[strategy as usize] += 1;
        c.detected += 1;
        if suppressed {
            c.suppressed += 1;
            c.resid_us += jitter_us;
            c.resid_n += 1;
        }
        c.buf_hist[buf_depth as usize] += 1;
        Ok(())
    }

    /// 查询单模式指标列（分列纪律：必须带模式码）。
    pub fn col(&self, mode: u8) -> Result<&JitterCol, u16> {
        let i = Self::idx(mode)?;
        Ok(&self.cols[i])
    }

    /// 跨模式总检测数（仅限遥测面板汇总使用，不替代分列查询）。
    pub fn total_detected(&self) -> u64 {
        self.cols.iter().map(|c| c.detected).sum()
    }

    /// B08 打包：12 word = magic + 模式 + 五指标面 + 校验字。
    /// 布局：[0]=MAGIC [1]=mode [2..6]=谱主桶计数（4 word：u64 拆对）
    ///       [6..8]=抑制率/残余均值 [8..10]=检测/抑制计数
    ///       [10]=策略命中合计 [11]=校验字（前 11 word 折叠异或）。
    pub fn encode(&self, mode: u8) -> Result<[u32; JT_WORDS], u16> {
        let c = self.col(mode)?;
        let dom = c.dominant_bucket() as u64;
        let mut w = [0u32; JT_WORDS];
        w[0] = JT_MAGIC;
        w[1] = mode as u32;
        w[2] = (c.spec[dom as usize] & 0xFFFF_FFFF) as u32;
        w[3] = (c.spec[dom as usize] >> 32) as u32;
        w[4] = (c.resid_mean_us() & 0xFFFF_FFFF) as u32;
        w[5] = (c.resid_mean_us() >> 32) as u32;
        w[6] = c.supp_rate_pm() as u32;
        w[7] = c.dominant_bucket() as u32;
        w[8] = (c.detected & 0xFFFF_FFFF) as u32;
        w[9] = (c.detected >> 32) as u32;
        w[10] = c.strat_hits.iter().sum::<u64>() as u32;
        let mut chk = JT_MAGIC ^ (mode as u32);
        for x in w.iter().take(11) {
            chk ^= *x;
        }
        w[11] = chk;
        Ok(w)
    }
}

/// B08 解码校验：还原（模式, 主桶号, 抑制率‰）。
pub fn decode(w: &[u32; JT_WORDS]) -> Result<(u8, u8, u64), u16> {
    if w[0] != JT_MAGIC {
        return Err(E_JT_MAGIC);
    }
    let mut chk = w[0] ^ w[1];
    for x in w.iter().take(11).skip(2) {
        chk ^= *x;
    }
    if chk != w[11] {
        return Err(E_JT_MAGIC);
    }
    let mode = w[1] as u8;
    if Self_::mode_valid(mode).is_err() {
        return Err(E_JT_MODE);
    }
    Ok((mode, w[7] as u8, w[6] as u64))
}

/// 内部小工具（避免与结构体方法纠缠的独立命名）。
struct Self_;
impl Self_ {
    fn mode_valid(mode: u8) -> Result<(), u16> {
        match mode {
            PM_VSYNC | PM_MAILBOX | PM_IMMEDIATE => Ok(()),
            _ => Err(E_JT_MODE),
        }
    }
}

// ==========================================================================
// 判据
// ==========================================================================

pub fn cgb506_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 1. 三模式分列：记录互不串列
    let mut t = JitterTelem::new();
    let _ = t.record(PM_VSYNC, 3_000, ST_DEEPBUF, true, 2);
    let _ = t.record(PM_MAILBOX, 5_000, ST_EARLYREN, false, 3);
    let ok_col = t.col(PM_VSYNC).map(|c| c.detected == 1).unwrap_or(false)
        && t.col(PM_MAILBOX).map(|c| c.detected == 1).unwrap_or(false)
        && t.col(PM_IMMEDIATE).map(|c| c.detected == 0).unwrap_or(false);
    v.push(("分列-三模式记录互不串列", ok_col));

    // 2. 谱桶归位：1.5ms 落 1 号桶，70ms 落末桶
    let mut t2 = JitterTelem::new();
    let _ = t2.record(PM_VSYNC, 1_500, ST_PASSTHRU, false, 0);
    let _ = t2.record(PM_VSYNC, 70_000, ST_PASSTHRU, false, 0);
    let ok_spec = t2
        .col(PM_VSYNC)
        .map(|c| c.spec[1] == 1 && c.spec[7] == 1 && c.spec[0] == 0)
        .unwrap_or(false);
    v.push(("五指标-谱桶边界归位", ok_spec));

    // 3. 抑制率 ‰ 口径：2/4 = 500
    let mut t3 = JitterTelem::new();
    for k in 0..4 {
        let _ = t3.record(PM_IMMEDIATE, 2_000, ST_COMBO, k < 2, 1);
    }
    let ok_rate = t3
        .col(PM_IMMEDIATE)
        .map(|c| c.supp_rate_pm() == 500)
        .unwrap_or(false);
    v.push(("五指标-抑制率千分比", ok_rate));

    // 4. 残余均值：抑制事件 (1000+3000)/2 = 2000
    let ok_resid = t3
        .col(PM_IMMEDIATE)
        .map(|c| c.resid_mean_us() == 2_000)
        .unwrap_or(false);
    v.push(("五指标-残余抖动均值", ok_resid));

    // 5. 非法输入拦截：模式/策略/深度三向
    let mut t4 = JitterTelem::new();
    let bad_mode = t4.record(9, 1_000, ST_PASSTHRU, false, 0) == Err(E_JT_MODE);
    let bad_strat = t4.record(PM_VSYNC, 1_000, 9, false, 0) == Err(E_JT_STRAT);
    let bad_depth = t4.record(PM_VSYNC, 1_000, ST_PASSTHRU, false, 9) == Err(E_JT_DEPTH);
    v.push(("拦截-非法模式策略深度", bad_mode && bad_strat && bad_depth));

    // 6. B08 往返：打包-解码一致，篡改即拒
    let w = t.encode(PM_MAILBOX).unwrap_or([0; JT_WORDS]);
    let dec = decode(&w);
    let ok_rt = dec == Ok((PM_MAILBOX, 2, 0)); // mailbox 记录 5000us→谱桶 2；未抑制→抑制率 0
    let mut w2 = w;
    w2[6] ^= 0xFF;
    v.push(("B08-往返一致且篡改拒", ok_rt && decode(&w2) == Err(E_JT_MAGIC)));

    // 7. 策略命中分账：passthru 与 combo 独立计数
    let ok_strat = t3
        .col(PM_IMMEDIATE)
        .map(|c| c.strat_hits[ST_COMBO as usize] == 4 && c.strat_hits[0] == 0)
        .unwrap_or(false);
    v.push(("分列-策略命中独立计数", ok_strat));

    // 8. 缓冲深度分布：深度 1 全落 1 号桶
    let ok_buf = t3
        .col(PM_IMMEDIATE)
        .map(|c| c.buf_hist[1] == 4 && c.buf_hist[0] == 0)
        .unwrap_or(false);
    v.push(("五指标-缓冲深度分布", ok_buf));

    v
}

pub fn run_cgb506_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("cgpu-cgb506");
    for (name, ok) in cgb506_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::cgb506_raw_checks as raw;
    #[test]
    fn t_cgb506() {
        for (n, ok) in raw() {
            assert!(ok, "cgb506 check failed: {}", n);
        }
    }
}
