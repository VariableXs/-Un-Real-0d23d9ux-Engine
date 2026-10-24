//! cgd24_verif — CGPU-F0504 抖动抑制验证协议（注入验证/发生器/矩阵/入册）。
//!
//! 判据原文（施工书锚点 CGPU-F0504）：验证协议：注入验证（已知抖动注入→
//! 抑制效果量化——抑制率/残余抖动量化）；注入器（可编程抖动发生器——测试
//! 专用）；验证矩阵（五类抖动源×三策略的抑制效果矩阵）；协议入册（CI 周
//! 期执行）。判据：量化、发生器、矩阵、入册、判据。
//!
//! 模型：可编程发生器按五类源产出确定性抖动序列；三策略分别抑制；抑制
//! 率 = 1 - 残余 stddev/注入 stddev（千分比）；5×3 矩阵全格实测，缺格即
//! 拒；入册记录带轮次号供 CI 对账。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 诊断码段 0x9100（cgpu 域内唯一）。
pub const XC_VFN_SRC_OOB: u32 = 0x9101;
pub const XC_VFN_POLICY_OOB: u32 = 0x9102;
pub const XC_VFN_MATRIX_HOLE: u32 = 0x9103;
pub const XC_VFN_REG_MISSING: u32 = 0x9104;
pub const XC_VFN_QUANT_BAD: u32 = 0x9105;

/// 五类抖动源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JitSource {
    /// S0 脉冲（偶发单帧长刺）。
    Pulse = 0,
    /// S1 周期性（节拍性起伏）。
    Periodic = 1,
    /// S2 渐增（缓慢漂移后回弹）。
    Ramp = 2,
    /// S3 随机（均匀随机）。随机（均匀随机）。
    Random = 3,
    /// S4 突发群（连续数帧密集）。
    Burst = 4,
}

/// 三抑制策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupPolicy {
    /// P0 深度缓冲。
    Depth = 0,
    /// P1 预测补偿。
    Predict = 1,
    /// P2 呈现重排。
    Resched = 2,
}

/// 可编程抖动发生器（测试专用，确定性）。
pub struct JitterGen {
    src: JitSource,
    amp_us: u32,
    seed: u32,
    step: u32,
}

impl JitterGen {
    /// 构造：幅度 1..=20000µs。
    pub fn new(src: JitSource, amp_us: u32, seed: u32) -> Result<Self, u32> {
        if amp_us == 0 || amp_us > 20_000 {
            return Err(XC_VFN_SRC_OOB);
        }
        Ok(JitterGen {
            src,
            amp_us,
            seed,
            step: 0,
        })
    }

    /// 线性同余伪随机（确定性）。
    fn lcg(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (self.seed >> 8) & 0xFFFF
    }

    /// 产出下一帧抖动（µs，无符号偏移量）。
    pub fn next_us(&mut self) -> u32 {
        let r = self.lcg();
        let v = match self.src {
            // 脉冲：每 8 帧一刺。
            JitSource::Pulse => {
                if self.step % 8 == 0 {
                    self.amp_us
                } else {
                    0
                }
            }
            // 周期：正弦近似——半周期满幅半周期零。
            JitSource::Periodic => {
                if (self.step / 4) % 2 == 0 {
                    self.amp_us
                } else {
                    0
                }
            }
            // 渐增：每 16 帧锯齿。
            JitSource::Ramp => self.amp_us * (self.step % 16) / 16,
            // 随机：0..amp。
            JitSource::Random => self.amp_us.min(r),
            // 突发群：每 12 帧连刺 3 帧。
            JitSource::Burst => {
                if self.step % 12 < 3 {
                    self.amp_us
                } else {
                    0
                }
            }
        };
        self.step += 1;
        v
    }
}

/// 序列统计：均值/标准差（µs，整数近似——stddev 用平均绝对偏差替代）。
pub struct SeqStat {
    /// 样本数。
    pub n: u32,
    sum: u64,
    mad_sum: u64,
    max: u32,
}

impl SeqStat {
    /// 空统计。
    pub fn new() -> Self {
        SeqStat {
            n: 0,
            sum: 0,
            mad_sum: 0,
            max: 0,
        }
    }

    /// 吸收一个样本。
    pub fn push(&mut self, us: u32) {
        self.n += 1;
        self.sum += us as u64;
        if us > self.max {
            self.max = us;
        }
        // mad_sum 暂存平方和近似：|x-mean| 需两遍——简化：先存 sum，收尾算。
        // 此处存 Σx² 供近似。
        self.mad_sum += (us as u64) * (us as u64);
    }

    /// 均值（µs）。
    pub fn mean(&self) -> u32 {
        if self.n == 0 {
            0
        } else {
            (self.sum / self.n as u64) as u32
        }
    }

    /// 残余度量：RMS（µs）——整数平方根近似（牛顿迭代）。
    pub fn rms(&self) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let mean_sq = (self.mad_sum / self.n as u64) as u64;
        let mean = self.mean() as u64;
        let var = mean_sq.saturating_sub(mean * mean);
        isqrt(var as u64) as u32
    }
}

impl Default for SeqStat {
    fn default() -> Self {
        Self::new()
    }
}

/// 整数平方根（牛顿法，无浮点）。
pub fn isqrt(x: u64) -> u64 {
    if x < 2 {
        return x;
    }
    let mut g = x;
    let mut n = (g + 1) / 2;
    while n < g {
        g = n;
        n = (g + x / g) / 2;
    }
    g
}

/// 一次抑制实验：注入序列 vs 抑制后序列。
pub fn quantify(injected: &SeqStat, suppressed: &SeqStat) -> Result<u32, u32> {
    if injected.n == 0 || suppressed.n == 0 || injected.n != suppressed.n {
        return Err(XC_VFN_QUANT_BAD);
    }
    let ri = injected.rms() as u64;
    let rs = suppressed.rms() as u64;
    if ri == 0 {
        return Err(XC_VFN_QUANT_BAD);
    }
    // 抑制率（千分比）= (1 - rs/ri) * 1000，下限 0。
    let rate = 1000u64.saturating_sub(rs * 1000 / ri);
    Ok(rate as u32)
}

/// 简单抑制器：深度缓冲策略 = 滑动均值；预测补偿 = 减去上一帧增量；
/// 呈现重排 = 相邻对调（大吃小）。
pub fn suppress(src_us: &[u32], p: SupPolicy) -> Result<Vec<u32>, u32> {
    if src_us.is_empty() {
        return Err(XC_VFN_QUANT_BAD);
    }
    let mut out = Vec::new();
    match p {
        SupPolicy::Depth => {
            // 滑窗 3 均值。
            let mut i = 0;
            while i < src_us.len() {
                let a = src_us[i];
                let b = if i > 0 { src_us[i - 1] } else { a };
                let c = if i + 1 < src_us.len() { src_us[i + 1] } else { a };
                out.push(((a as u64 + b as u64 + c as u64) / 3) as u32);
                i += 1;
            }
        }
        SupPolicy::Predict => {
            // 一阶预测消偏：out[i] = x[i] - (x[i]-x[i-1])/2（饱和）。
            let mut i = 0;
            while i < src_us.len() {
                if i == 0 {
                    out.push(src_us[0]);
                } else {
                    let diff = src_us[i] as i64 - src_us[i - 1] as i64;
                    let v = src_us[i] as i64 - diff / 2;
                    out.push(v.max(0) as u32);
                }
                i += 1;
            }
        }
        SupPolicy::Resched => {
            // 相邻对调：偶位与奇位比较，大者排前（削峰）。
            let mut i = 0;
            while i < src_us.len() {
                if i + 1 < src_us.len() && src_us[i] < src_us[i + 1] {
                    out.push(src_us[i + 1]);
                    out.push(src_us[i]);
                    i += 2;
                } else {
                    out.push(src_us[i]);
                    i += 1;
                }
            }
        }
    }
    Ok(out)
}

/// 5×3 验证矩阵：全格实测，缺格即拒。
#[derive(Debug)]
pub struct VerifMatrix {
    cells: [[Option<u32>; 3]; 5],
}

impl VerifMatrix {
    /// 空矩阵。
    pub fn new() -> Self {
        VerifMatrix {
            cells: [[None; 3]; 5],
        }
    }

    /// 写格。
    pub fn put(&mut self, s: JitSource, p: SupPolicy, rate_permille: u32) -> Result<(), u32> {
        self.cells[s as usize][p as usize] = Some(rate_permille.min(1000));
        Ok(())
    }

    /// 读格；空格拒。
    pub fn get(&self, s: JitSource, p: SupPolicy) -> Result<u32, u32> {
        self.cells[s as usize][p as usize].ok_or(XC_VFN_MATRIX_HOLE)
    }

    /// 全格实测对账：15 格无一空。
    pub fn is_full(&self) -> bool {
        let mut r = 0;
        while r < 5 {
            let mut c = 0;
            while c < 3 {
                if self.cells[r][c].is_none() {
                    return false;
                }
                c += 1;
            }
            r += 1;
        }
        true
    }
}

impl Default for VerifMatrix {
    fn default() -> Self {
        Self::new()
    }
}

/// CI 入册记录。
#[derive(Clone, Copy, Debug)]
pub struct RegEntry {
    /// 轮次号。
    pub round: u32,
    /// 全格满标志。
    pub full: bool,
    /// 全矩阵最低抑制率（千分比）。
    pub min_rate: u32,
}

/// 协议入册账。
#[derive(Debug)]
pub struct RegBook {
    entries: Vec<RegEntry>,
    next_round: u32,
}

impl RegBook {
    /// 空账，轮次从 1 起。
    pub fn new() -> Self {
        RegBook {
            entries: Vec::new(),
            next_round: 1,
        }
    }

    /// 入册：矩阵不满拒。
    pub fn register(&mut self, m: &VerifMatrix) -> Result<&RegEntry, u32> {
        if !m.is_full() {
            return Err(XC_VFN_REG_MISSING);
        }
        let mut min_rate = 1000u32;
        let mut r = 0;
        while r < 5 {
            let mut c = 0;
            while c < 3 {
                let v = m.cells[r][c].unwrap_or(0);
                if v < min_rate {
                    min_rate = v;
                }
                c += 1;
            }
            r += 1;
        }
        let e = RegEntry {
            round: self.next_round,
            full: true,
            min_rate,
        };
        self.next_round += 1;
        self.entries.push(e);
        Ok(self.entries.last().unwrap())
    }

    /// 账目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 轮次连续对账：round 必须严格 1..n。
    pub fn rounds_contiguous(&self) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].round != (i + 1) as u32 {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl Default for RegBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 验证协议总门面。
#[derive(Debug, Default)]
pub struct VerifEngine {
    /// 矩阵。
    pub matrix: VerifMatrix,
    /// 入册账。
    pub book: RegBook,
}

impl VerifEngine {
    /// 自检一：发生器确定性——同种子同序列、五源产出非全零。
    pub fn selfcheck_gen(&self) -> Result<(), u32> {
        let mut g1 = JitterGen::new(JitSource::Pulse, 8000, 7)?;
        let mut g2 = JitterGen::new(JitSource::Pulse, 8000, 7)?;
        let mut i = 0;
        while i < 16 {
            if g1.next_us() != g2.next_us() {
                return Err(XC_VFN_SRC_OOB);
            }
            i += 1;
        }
        // 五源逐个跑 32 帧均有非零产出。
        let srcs = [
            JitSource::Pulse,
            JitSource::Periodic,
            JitSource::Ramp,
            JitSource::Random,
            JitSource::Burst,
        ];
        let mut s = 0;
        while s < srcs.len() {
            let mut g = JitterGen::new(srcs[s], 5000, 42)?;
            let mut nonzero = false;
            let mut f = 0;
            while f < 32 {
                if g.next_us() > 0 {
                    nonzero = true;
                }
                f += 1;
            }
            if !nonzero {
                return Err(XC_VFN_SRC_OOB);
            }
            s += 1;
        }
        if JitterGen::new(JitSource::Pulse, 0, 1).is_ok()
            || JitterGen::new(JitSource::Pulse, 20_001, 1).is_ok()
        {
            return Err(XC_VFN_SRC_OOB);
        }
        Ok(())
    }

    /// 自检二：量化——抑制率在 0..=1000、RMS 单调性、样本不齐拒。
    pub fn selfcheck_quant(&self) -> Result<(), u32> {
        let mut inj = SeqStat::new();
        let mut sup = SeqStat::new();
        let mut i = 0;
        while i < 40 {
            let v = if i % 8 == 0 { 9000 } else { 200 };
            inj.push(v);
            sup.push(if i % 8 == 0 { 4000 } else { 200 });
            i += 1;
        }
        let rate = quantify(&inj, &sup)?;
        if rate == 0 || rate > 1000 {
            return Err(XC_VFN_QUANT_BAD);
        }
        // 残余更大→抑制率 0（饱和）。
        let rate2 = quantify(&sup, &inj)?;
        if rate2 != 0 {
            return Err(XC_VFN_QUANT_BAD);
        }
        // 样本数不齐拒。
        let mut half = SeqStat::new();
        half.push(1);
        if quantify(&inj, &half).is_ok() {
            return Err(XC_VFN_QUANT_BAD);
        }
        Ok(())
    }

    /// 自检三：三策略实抑制——注入后残余 RMS 必须低于原始（深度/预测）。
    pub fn selfcheck_policies(&self) -> Result<(), u32> {
        let mut seq = Vec::new();
        let mut i = 0;
        while i < 32 {
            seq.push(if i % 4 == 0 { 8000 } else { 300 });
            i += 1;
        }
        let mut raw = SeqStat::new();
        for v in &seq {
            raw.push(*v);
        }
        let p_depth = suppress(&seq, SupPolicy::Depth)?;
        let mut sd = SeqStat::new();
        for v in &p_depth {
            sd.push(*v);
        }
        if sd.rms() >= raw.rms() {
            return Err(XC_VFN_QUANT_BAD);
        }
        let p_pred = suppress(&seq, SupPolicy::Predict)?;
        let mut sp = SeqStat::new();
        for v in &p_pred {
            sp.push(*v);
        }
        if sp.rms() >= raw.rms() {
            return Err(XC_VFN_QUANT_BAD);
        }
        let p_res = suppress(&seq, SupPolicy::Resched)?;
        if p_res.len() != seq.len() {
            return Err(XC_VFN_QUANT_BAD);
        }
        if suppress(&[], SupPolicy::Depth).is_ok() {
            return Err(XC_VFN_QUANT_BAD);
        }
        Ok(())
    }

    /// 自检四：矩阵全格实测——5 源×3 策略逐格产出并写入，缺格拒。
    pub fn selfcheck_matrix(&mut self) -> Result<(), u32> {
        let srcs = [
            JitSource::Pulse,
            JitSource::Periodic,
            JitSource::Ramp,
            JitSource::Random,
            JitSource::Burst,
        ];
        let pols = [SupPolicy::Depth, SupPolicy::Predict, SupPolicy::Resched];
        let mut s = 0;
        while s < srcs.len() {
            let mut g = JitterGen::new(srcs[s], 6000, 100 + s as u32)?;
            let mut raw = Vec::new();
            let mut f = 0;
            while f < 48 {
                raw.push(g.next_us());
                f += 1;
            }
            let mut stat_raw = SeqStat::new();
            for v in &raw {
                stat_raw.push(*v);
            }
            let mut p = 0;
            while p < pols.len() {
                let out = suppress(&raw, pols[p])?;
                let mut stat_out = SeqStat::new();
                for v in &out {
                    stat_out.push(*v);
                }
                let rate = quantify(&stat_raw, &stat_out)?;
                self.matrix.put(srcs[s], pols[p], rate)?;
                p += 1;
            }
            s += 1;
        }
        if !self.matrix.is_full() {
            return Err(XC_VFN_MATRIX_HOLE);
        }
        // 空格读取拒。
        let mut empty = VerifMatrix::new();
        if empty.get(JitSource::Pulse, SupPolicy::Depth).is_ok() {
            return Err(XC_VFN_MATRIX_HOLE);
        }
        let _ = empty;
        Ok(())
    }

    /// 自检五：入册——轮次连续、最低率对账、缺格拒入。
    pub fn selfcheck_register(&mut self) -> Result<(), u32> {
        if self.book.register(&VerifMatrix::new()).is_ok() {
            return Err(XC_VFN_REG_MISSING);
        }
        let e = self.book.register(&self.matrix)?;
        if e.round != 1 || !e.full || e.min_rate == 0 && e.min_rate > 1000 {
            return Err(XC_VFN_REG_MISSING);
        }
        let e2 = self.book.register(&self.matrix)?;
        if e2.round != 2 {
            return Err(XC_VFN_REG_MISSING);
        }
        if !self.book.rounds_contiguous() || self.book.len() != 2 {
            return Err(XC_VFN_REG_MISSING);
        }
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x9100..0x9200 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_VFN_SRC_OOB,
            XC_VFN_POLICY_OOB,
            XC_VFN_MATRIX_HOLE,
            XC_VFN_REG_MISSING,
            XC_VFN_QUANT_BAD,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x9100 || codes[i] >= 0x9200 {
                return Err(codes[i]);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(codes[i]);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgd24_selfchecks() {
        let mut e = VerifEngine::default();
        e.selfcheck_gen().unwrap();
        e.selfcheck_quant().unwrap();
        e.selfcheck_policies().unwrap();
        e.selfcheck_matrix().unwrap();
        e.selfcheck_register().unwrap();
        assert_eq!(VerifEngine::segment_guard(), Ok(()));
    }
}
