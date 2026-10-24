//! cgd27_jfuzz — CGPU-F0507 抖动 fuzz（间隔异常注入/参数钳制/不变量/CI 固化）。
//!
//! 判据原文（施工书锚点 CGPU-F0507）：fuzz：间隔异常注入 fuzz（极端间隔/
//! 负跳变/超长帧——均衡器容错不变量：抑制不增加延迟上限/不丢帧序）；策略
//! 参数 fuzz（非法参数钳制）；案例固化 CI。判据：异常注入、不变量、CI、
//! 判据。
//!
//! 模型：三类异常注入器（极端间隔=0/超长帧=大值/负跳变=序列骤降）驱动均
//! 衡器；不变量两条硬断言（输出序列长度=输入序列长度【不丢帧序】、输出最
//! 大值 ≤ 输入最大值【抑制不增延迟上限】）；策略参数 fuzz 非法值一律钳制
//! 合法域；固化为可重复回放的确定性案例账供 CI。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 诊断码段 0x9C00（cgpu 域内唯一）。
pub const XC_FZ_INV_ORDER: u32 = 0x9C01;
pub const XC_FZ_INV_CEILING: u32 = 0x9C02;
pub const XC_FZ_PARAM_UNCLAMPED: u32 = 0x9C03;
pub const XC_FZ_CASE_MISSING: u32 = 0x9C04;
pub const XC_FZ_INJ_EMPTY: u32 = 0x9C05;

/// 三类异常注入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anomaly {
    /// 极端间隔：0µs 帧（超短帧）。
    ZeroGap,
    /// 超长帧：大值帧。
    LongFrame,
    /// 负跳变：序列骤降。
    NegativeJump,
}

/// 生成异常序列：基线 300µs 每帧，按 anomaly 替换。
pub fn inject(n: usize, a: Anomaly, seed: u32) -> Result<Vec<u32>, u32> {
    if n == 0 {
        return Err(XC_FZ_INJ_EMPTY);
    }
    let mut s = seed | 1;
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let r = (s >> 8) & 0xFFFF;
        let v = match a {
            Anomaly::ZeroGap => {
                if r % 7 == 0 {
                    0
                } else {
                    300
                }
            }
            Anomaly::LongFrame => {
                if r % 7 == 0 {
                    19_000
                } else {
                    300
                }
            }
            Anomaly::NegativeJump => {
                if r % 7 == 0 {
                    // 从高位骤降。
                    if i > 0 && out[i - 1] > 500 {
                        10
                    } else {
                        5_000
                    }
                } else {
                    300
                }
            }
        };
        out.push(v);
        i += 1;
    }
    Ok(out)
}

/// 均衡器（fuzz 靶）：三帧滑窗均值 + 长帧钳制（>8000µs 视异常钳到 8000）。
pub fn balance(input: &[u32]) -> Result<Vec<u32>, u32> {
    if input.is_empty() {
        return Err(XC_FZ_INJ_EMPTY);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < input.len() {
        let a = input[i].min(8000);
        let b = if i > 0 { input[i - 1].min(8000) } else { a };
        let c = if i + 1 < input.len() { input[i + 1].min(8000) } else { a };
        out.push(((a as u64 + b as u64 + c as u64) / 3) as u32);
        i += 1;
    }
    Ok(out)
}

/// 不变量一：不丢帧序——输出长度==输入长度且逐帧非负（结构守恒）。
pub fn inv_order(input: &[u32], out: &[u32]) -> bool {
    input.len() == out.len()
}

/// 不变量二：抑制不增加延迟上限——输出最大 ≤ 输入最大。
pub fn inv_ceiling(input: &[u32], out: &[u32]) -> bool {
    let mut mi = 0u32;
    let mut mo = 0u32;
    let mut i = 0;
    while i < input.len() {
        if input[i] > mi {
            mi = input[i];
        }
        i += 1;
    }
    let mut i = 0;
    while i < out.len() {
        if out[i] > mo {
            mo = out[i];
        }
        i += 1;
    }
    mo <= mi
}

/// 策略参数（fuzz 钳制靶）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BalParam {
    /// 滑窗宽 1..=7。
    pub window: u8,
    /// 长帧钳制阈值 1000..=8000µs。
    pub clamp_us: u32,
}

impl BalParam {
    /// 合法域钳制：window 1..=7、clamp 1000..=8000。
    pub fn clamp(window: u8, clamp_us: u32) -> Self {
        BalParam {
            window: window.clamp(1, 7),
            clamp_us: clamp_us.clamp(1000, 8000),
        }
    }

    /// 是否本来合法（未触发钳制）。（const fn 显式比较——contains 尚未 const 稳定。）
    pub const fn was_valid(window: u8, clamp_us: u32) -> bool {
        window >= 1 && window <= 7 && clamp_us >= 1000 && clamp_us <= 8000
    }
}

/// 固化案例（CI 回放账）。
#[derive(Clone, Debug)]
pub struct FuzzCase {
    /// 注入类型。
    pub anomaly: Anomaly,
    /// 帧数。
    pub frames: usize,
    /// 种子。
    pub seed: u32,
    /// 回放时输出长度。
    pub out_len: usize,
    /// 回放时输出最大值。
    pub out_max: u32,
}

/// 案例账。
#[derive(Debug)]
pub struct CaseBook {
    cases: Vec<FuzzCase>,
}

impl CaseBook {
    /// 空账。
    pub fn new() -> Self {
        CaseBook { cases: Vec::new() }
    }

    /// 固化一次运行。
    pub fn freeze(&mut self, anomaly: Anomaly, frames: usize, seed: u32) -> Result<&FuzzCase, u32> {
        let seq = inject(frames, anomaly, seed)?;
        let out = balance(&seq)?;
        if !inv_order(&seq, &out) || !inv_ceiling(&seq, &out) {
            if !inv_order(&seq, &out) {
                return Err(XC_FZ_INV_ORDER);
            }
            return Err(XC_FZ_INV_CEILING);
        }
        let mut mx = 0u32;
        let mut i = 0;
        while i < out.len() {
            if out[i] > mx {
                mx = out[i];
            }
            i += 1;
        }
        let c = FuzzCase {
            anomaly,
            frames,
            seed,
            out_len: out.len(),
            out_max: mx,
        };
        self.cases.push(c);
        Ok(self.cases.last().unwrap())
    }

    /// CI 回放：按账目重跑并逐字段对账。
    pub fn replay(&self) -> Result<usize, u32> {
        if self.cases.is_empty() {
            return Err(XC_FZ_CASE_MISSING);
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.cases.len() {
            let c = &self.cases[i];
            let seq = inject(c.frames, c.anomaly, c.seed)?;
            let out = balance(&seq)?;
            if out.len() != c.out_len {
                return Err(XC_FZ_INV_ORDER);
            }
            let mut mx = 0u32;
            for v in &out {
                if *v > mx {
                    mx = *v;
                }
            }
            if mx != c.out_max {
                return Err(XC_FZ_INV_CEILING);
            }
            ok += 1;
            i += 1;
        }
        Ok(ok)
    }

    /// 账目数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }
}

impl Default for CaseBook {
    fn default() -> Self {
        Self::new()
    }
}

/// fuzz 总门面。
#[derive(Debug, Default)]
pub struct JFuzzEngine {
    /// 案例账。
    pub book: CaseBook,
}

impl JFuzzEngine {
    /// 自检一：三类异常注入——序列非空、异常帧确实存在、确定性。
    pub fn selfcheck_inject(&self) -> Result<(), u32> {
        let kinds = [Anomaly::ZeroGap, Anomaly::LongFrame, Anomaly::NegativeJump];
        let mut k = 0;
        while k < kinds.len() {
            let a = inject(32, kinds[k], 9)?;
            let b = inject(32, kinds[k], 9)?;
            if a != b {
                return Err(XC_FZ_INJ_EMPTY);
            }
            // 异常帧存在性。
            let mut hit = false;
            let mut i = 0;
            while i < a.len() {
                match kinds[k] {
                    Anomaly::ZeroGap => {
                        if a[i] == 0 {
                            hit = true;
                        }
                    }
                    Anomaly::LongFrame => {
                        if a[i] == 19_000 {
                            hit = true;
                        }
                    }
                    Anomaly::NegativeJump => {
                        if i > 0 && a[i - 1] > 500 && a[i] == 10 {
                            hit = true;
                        }
                    }
                }
                i += 1;
            }
            if !hit {
                return Err(XC_FZ_INJ_EMPTY);
            }
            k += 1;
        }
        if inject(0, Anomaly::ZeroGap, 1).is_ok() {
            return Err(XC_FZ_INJ_EMPTY);
        }
        Ok(())
    }

    /// 自检二：不变量——三类异常 × 多种子全部守恒；构造违规靶验证检出。
    pub fn selfcheck_invariants(&self) -> Result<(), u32> {
        let kinds = [Anomaly::ZeroGap, Anomaly::LongFrame, Anomaly::NegativeJump];
        let mut seed = 1;
        while seed <= 5 {
            let mut k = 0;
            while k < kinds.len() {
                let seq = inject(48, kinds[k], seed)?;
                let out = balance(&seq)?;
                if !inv_order(&seq, &out) {
                    return Err(XC_FZ_INV_ORDER);
                }
                if !inv_ceiling(&seq, &out) {
                    return Err(XC_FZ_INV_CEILING);
                }
                k += 1;
            }
            seed += 1;
        }
        // 违规靶：伪输出长度不符→检出。
        let seq = inject(8, Anomaly::ZeroGap, 3)?;
        let out = balance(&seq)?;
        if inv_order(&seq, &out[..out.len() - 1]) {
            return Err(XC_FZ_INV_ORDER);
        }
        // 伪输出抬顶→检出。
        let mut bad = out.clone();
        bad[0] = seq.iter().copied().max().unwrap_or(0) + 1;
        if inv_ceiling(&seq, &bad) {
            return Err(XC_FZ_INV_CEILING);
        }
        // 空输入拒。
        if balance(&[]).is_ok() {
            return Err(XC_FZ_INJ_EMPTY);
        }
        Ok(())
    }

    /// 自检三：参数钳制——非法域全部落回合法域。
    pub fn selfcheck_clamp(&self) -> Result<(), u32> {
        let pairs: [(u8, u32); 8] = [
            (0, 0),
            (8, 9000),
            (255, 99),
            (3, 500),
            (7, 8000),
            (1, 1000),
            (2, 200_000),
            (4, 4000),
        ];
        let mut i = 0;
        while i < pairs.len() {
            let (w, c) = pairs[i];
            let p = BalParam::clamp(w, c);
            if !(1..=7).contains(&p.window) || !(1000..=8000).contains(&p.clamp_us) {
                return Err(XC_FZ_PARAM_UNCLAMPED);
            }
            // 本来合法的输入必须原样保留（不得误钳）。
            if BalParam::was_valid(w, c) && (p.window != w || p.clamp_us != c) {
                return Err(XC_FZ_PARAM_UNCLAMPED);
            }
            i += 1;
        }
        Ok(())
    }

    /// 自检四：案例固化——3 类 × 2 种子 = 6 例入账。
    pub fn selfcheck_freeze(&mut self) -> Result<(), u32> {
        let kinds = [Anomaly::ZeroGap, Anomaly::LongFrame, Anomaly::NegativeJump];
        let mut k = 0;
        while k < kinds.len() {
            let mut s = 0;
            while s < 2 {
                self.book.freeze(kinds[k], 40, 100 + s)?;
                s += 1;
            }
            k += 1;
        }
        if self.book.len() != 6 {
            return Err(XC_FZ_CASE_MISSING);
        }
        Ok(())
    }

    /// 自检五：CI 回放——六例全对账、空账拒、判据收口。
    pub fn selfcheck_replay(&mut self) -> Result<(), u32> {
        if CaseBook::new().replay().is_ok() {
            return Err(XC_FZ_CASE_MISSING);
        }
        let n = self.book.replay()?;
        if n != 6 {
            return Err(XC_FZ_CASE_MISSING);
        }
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x9C00..0x9D00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_FZ_INV_ORDER,
            XC_FZ_INV_CEILING,
            XC_FZ_PARAM_UNCLAMPED,
            XC_FZ_CASE_MISSING,
            XC_FZ_INJ_EMPTY,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x9C00 || codes[i] >= 0x9D00 {
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
    fn cgd27_selfchecks() {
        let mut e = JFuzzEngine::default();
        e.selfcheck_inject().unwrap();
        e.selfcheck_invariants().unwrap();
        e.selfcheck_clamp().unwrap();
        e.selfcheck_freeze().unwrap();
        e.selfcheck_replay().unwrap();
        assert_eq!(JFuzzEngine::segment_guard(), Ok(()));
    }
}
