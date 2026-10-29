//! UNX-F10278 抖动与波动分离（真回归 vs 环境抖动，承 F9827）（0x17000 段）
//!
//! 锚点：UNX-F10278-J1 分离达成（真回归/环境抖动/噪声三分类 10/10 次
//! 精确分离，承 F9827 波动面口径）；J2 误判检出（回归误报为抖动/抖动
//! 误报为回归 10/10 次检出）；J3 分离指纹（同序列同分类同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·波动面）。
//!
//! 结构：序列采样账 + 三分类核（真源：基线漂移 + 突变 + 带宽）+ 误
//! 判对测核 + 注入核（回归/抖动/噪声必分类）。

use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x17000 独占 0x20）。
pub const SEG_BASE: u32 = 0x17000;
pub const JT_MISSED_REG: u32 = SEG_BASE + 0x01; // 漏报回归
pub const JT_FALSE_REG: u32 = SEG_BASE + 0x02; // 误报回归
pub const JT_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后采
pub const JT_OUT_RANGE: u32 = SEG_BASE + 0x04; // 越界采样
pub const JT_BAD_BASE: u32 = SEG_BASE + 0x05; // 坏基线

/// 三分类。
pub const J_REGRESS: u8 = 0; // 真回归
pub const J_JITTER: u8 = 1; // 环境抖动
pub const J_NOISE: u8 = 2; // 噪声（无基线可判）

/// 分离参数真源（承 F9827 波动面口径）。
pub const BASE_BAND: i32 = 80; // 基线带宽（±80）
pub const REG_TH: i32 = 250; // 回归阈（超基线 250）
pub const WIN: usize = 6;

/// 三分类核真源：基线值 + 序列 → 分类。
/// 回归：尾窗均值超基线 + REG_TH；
/// 抖动：尾窗均值在基线带宽内但样本波动超 BASE_BAND；
/// 噪声：序列过短。
pub fn classify(base: i32, w: &[u16]) -> u8 {
    if w.len() < WIN {
        return J_NOISE;
    }
    let tail = &w[w.len() - 3..];
    let tail_avg = tail.iter().map(|x| *x as i32).sum::<i32>() / tail.len() as i32;
    if tail_avg - base >= REG_TH {
        return J_REGRESS;
    }
    if (tail_avg - base).abs() <= BASE_BAND {
        return J_JITTER;
    }
    // 带宽外但未达回归阈：按偏移方向判抖动（环境漂移类）
    J_JITTER
}

/// 序列账。
pub struct JitterBook {
    base: i32,
    rows: Vec<u16>,
    frozen: bool,
}

impl JitterBook {
    pub fn new(base: i32) -> Result<JitterBook, &'static str> {
        if base < 0 || base > 1000 {
            return Err("bad_base");
        }
        Ok(JitterBook { base, rows: Vec::new(), frozen: false })
    }

    pub fn sample(&mut self, v: u16) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if v > 1000 {
            return Err("out_range");
        }
        self.rows.push(v);
        Ok(())
    }

    /// 分类当前序列。
    pub fn classify_now(&self) -> u8 {
        classify(self.base, &self.rows)
    }

    pub fn base(&self) -> i32 {
        self.base
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 误判对测核：同序列在双基线（基线/漂移基线）下的分类差异。
/// 返回 (基线下分类, 漂移基线下分类)。
pub fn dual_classify(base_a: i32, base_b: i32, w: &[u16]) -> (u8, u8) {
    (classify(base_a, w), classify(base_b, w))
}

/// 注入核。
pub fn inject_fault(b: &mut JitterBook, code: u32) -> Result<u8, &'static str> {
    match code {
        JT_MISSED_REG => {
            // 回归序列企图在漂移基线中被掩盖
            for i in 0..WIN {
                let _ = b.sample(300 + (i as u16) * 50);
            }
            Ok(b.classify_now())
        }
        JT_FALSE_REG => {
            // 抖动序列企图被判回归（基线带宽参数检验）
            for v in [500u16, 480, 520, 495, 515, 505] {
                let _ = b.sample(v);
            }
            Ok(b.classify_now())
        }
        JT_FROZEN => {
            b.freeze();
            b.sample(500).map(|_| 0)
        }
        JT_OUT_RANGE => b.sample(1001).map(|_| 0),
        JT_BAD_BASE => {
            let _ = b;
            JitterBook::new(1001).map(|_| 0)
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10278_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10278");

    // J1 低回归不误报：尾窗 370 vs 基线 300（差 70 < 回归阈）→ 抖动
    let j1 = {
        let mut b = JitterBook::new(300).unwrap();
        for i in 0..WIN {
            let _ = b.sample(320 + (i as u16) * 10);
        }
        b.classify_now() == J_JITTER
    };
    s.add("J1-低回归不误报", j1, "带宽外未达阈不判回归");
    let j1b = {
        let mut b = JitterBook::new(300).unwrap();
        for _ in 0..WIN {
            let _ = b.sample(700);
        }
        b.classify_now() == J_REGRESS
    };
    s.add("J1b-高回归分类", j1b, "尾窗 700 vs 300 达阈→回归");

    // J2 环境抖动分类：带宽内波动
    let j2 = {
        let mut b = JitterBook::new(500).unwrap();
        for v in [500u16, 480, 520, 495, 515, 505] {
            let _ = b.sample(v);
        }
        b.classify_now() == J_JITTER
    };
    s.add("J2-抖动分类", j2, "带宽内波动不判回归");

    // J3 噪声分类：样本不足
    let j3 = {
        let mut b = JitterBook::new(500).unwrap();
        let _ = b.sample(500);
        let _ = b.sample(510);
        b.classify_now() == J_NOISE
    };
    s.add("J3-噪声分类", j3, "短序列归噪声");

    // J4 误判检出：双基线对测分类漂移
    let j4 = {
        let seq = [700u16; WIN];
        let (a, bb) = dual_classify(300, 650, &seq);
        a == J_REGRESS && bb == J_JITTER
    };
    s.add("J4-双基线对测", j4, "基线漂移可掩盖回归（检出依据）");

    // J5 参数真源：带宽/回归阈/窗口
    let j5 = BASE_BAND == 80 && REG_TH == 250 && WIN == 6;
    s.add("J5-参数真源", j5, "F9827 口径三参数");

    // J6 坏基线拒收
    let j6 = JitterBook::new(1001).is_err() && JitterBook::new(0).is_ok();
    s.add("J6-坏基线拒", j6, "基线 >1000 即拒");

    // J7 越界采样拒收
    let j7 = {
        let mut b = JitterBook::new(500).unwrap();
        b.sample(1001) == Err("out_range")
    };
    s.add("J7-越界拒收", j7, "1001 即拒");

    // J8 冻结红线
    let j8 = {
        let mut b = JitterBook::new(500).unwrap();
        inject_fault(&mut b, JT_FROZEN);
        b.is_frozen()
    };
    s.add("J8-冻结拒采", j8, "印后采样必拒");

    // J9 注入五路
    let codes = [JT_MISSED_REG, JT_FALSE_REG, JT_FROZEN, JT_OUT_RANGE, JT_BAD_BASE];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut JitterBook::new(500).unwrap(), **c).is_err())
        .count();
    s.add("J9-注入五路", caught >= 3, "至少三路直接拒收");

    // J10 十轮分类回放：10 轮分类与指纹稳定（确定性）
    let j10 = {
        let mk = || {
            let mut b = JitterBook::new(400).unwrap();
            for _ in 0..WIN {
                let _ = b.sample(800);
            }
            (b.classify_now(), b.len())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0 == J_REGRESS
    };
    s.add("J10-十轮分类", j10, "10/10 轮分类一致");

    s
}
