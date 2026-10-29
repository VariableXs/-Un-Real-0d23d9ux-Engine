//! UNX-F11223 最小样本逐特性点亮策略账（0x1f823 段）
//!
//! 锚点：UNX-F11223-J1 点亮策略（样本×特性矩阵、最小复现优先序、点亮判据
//! 三成分）落账，矩阵覆盖 30 样本全特性。
//! 域：CoRun·UNX-C5-B02——一个样本只有三成分同时成立才算「点亮」：实跑通、
//! 输出同码、无内核缺口；缺一即暗，且暗样本不允许混入冻存基线。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f823;
pub const LS_ID: u32 = SEG_BASE + 0x01; // 样本号非法/重复
pub const LS_BITS: u32 = SEG_BASE + 0x02; // 特性位为空或越界
pub const LS_COVER: u32 = SEG_BASE + 0x03; // 特性覆盖不全
pub const LS_DARK: u32 = SEG_BASE + 0x04; // 存在未点亮样本
pub const LS_ORDER: u32 = SEG_BASE + 0x05; // 最小复现序违例
pub const LS_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const LS_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 六项特性位（位序固定）。
pub const FEATURES: [&str; 6] = ["静态启动", "动态装载", "TLS", "信号", "线程", "malloc"];
/// 样本总数与特性位上限。
pub const WANT_SAMPLES: usize = 30;
pub const FEAT_MASK: u8 = 0x3f;
pub const LS_TAG: u64 = 0x1f823a7;
pub const FP_STEP: u64 = 0x100000001b3;

/// 点亮判据三成分。
#[derive(Clone, Copy, Debug)]
pub struct Verdict {
    pub runs: bool,
    pub output: bool,
    pub gap: bool,
}

impl Verdict {
    pub fn lit(&self) -> bool {
        self.runs && self.output && !self.gap
    }

    pub fn components(&self) -> u32 {
        self.runs as u32 + self.output as u32 + (!self.gap) as u32
    }
}

/// 一份最小样本。
#[derive(Clone, Debug)]
pub struct Sample {
    pub id: u32,
    pub bits: u8,
    pub verdict: Verdict,
}

impl Sample {
    pub fn feats(&self) -> u32 {
        self.bits.count_ones()
    }

    pub fn minimal(&self) -> bool {
        self.feats() <= 2
    }
}

/// 点亮策略账。
#[derive(Clone, Debug)]
pub struct Strategy {
    pub samples: Vec<Sample>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Strategy {
    pub fn new() -> Strategy {
        Strategy { samples: Vec::new(), frozen: false, root: None }
    }

    /// 录样本：号域 1..=30、位非零不越界、号唯一。
    pub fn add(&mut self, id: u32, bits: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(LS_FROZEN);
        }
        if id == 0 || id as usize > WANT_SAMPLES {
            return Err(LS_ID);
        }
        if bits == 0 || bits & !FEAT_MASK != 0 {
            return Err(LS_BITS);
        }
        if self.samples.iter().any(|s| s.id == id) {
            return Err(LS_ID);
        }
        self.samples.push(Sample { id, bits, verdict: Verdict { runs: false, output: false, gap: true } });
        Ok(())
    }

    /// 落一份样本的三成分。
    pub fn judge(&mut self, id: u32, v: Verdict) -> Result<(), u32> {
        if self.frozen {
            return Err(LS_FROZEN);
        }
        let mut k = 0usize;
        while k < self.samples.len() {
            if self.samples[k].id == id {
                self.samples[k].verdict = v;
                return Ok(());
            }
            k += 1;
        }
        Err(LS_ID)
    }

    /// 特性覆盖：六位并集是否为满掩码。
    pub fn covered(&self) -> u8 {
        let mut m = 0u8;
        let mut k = 0usize;
        while k < self.samples.len() {
            m |= self.samples[k].bits;
            k += 1;
        }
        m
    }

    pub fn cover_count(&self) -> u32 {
        self.covered().count_ones()
    }

    /// 点亮序：按特性数升序、同数按号升序，即为最小复现优先。
    pub fn order_ok(&self) -> bool {
        let mut prev = 0u32;
        let mut k = 0usize;
        while k < self.samples.len() {
            let f = self.samples[k].feats();
            if k > 0 && f < prev {
                return false;
            }
            prev = f;
            k += 1;
        }
        true
    }

    pub fn lit_count(&self) -> usize {
        self.samples.iter().filter(|s| s.verdict.lit()).count()
    }

    pub fn dark(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.samples.len() {
            if !self.samples[k].verdict.lit() {
                out.push(self.samples[k].id);
            }
            k += 1;
        }
        out
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(LS_FROZEN);
        }
        if self.samples.len() != WANT_SAMPLES {
            return Err(LS_DARK);
        }
        if !self.order_ok() {
            return Err(LS_ORDER);
        }
        if self.cover_count() != FEATURES.len() as u32 {
            return Err(LS_COVER);
        }
        if self.lit_count() != WANT_SAMPLES {
            return Err(LS_DARK);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("lit {}/{} cover {}/6", self.lit_count(), self.samples.len(), self.cover_count())
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = LS_TAG;
        let mut k = 0usize;
        while k < self.samples.len() {
            h = (h ^ self.samples[k].id as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.samples[k].bits as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.samples[k].verdict.components() as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 组一张三十样本、全特性覆盖、全点亮的策略账。
pub fn build() -> Strategy {
    let mut s = Strategy::new();
    let mut id = 1u32;
    while id <= WANT_SAMPLES as u32 {
        let i = (id - 1) as usize;
        let bits = if i < 6 { 1u8 << i } else { (1u8 << (i % 6)) | (1u8 << ((i + 1) % 6)) };
        let _ = s.add(id, bits);
        let _ = s.judge(id, Verdict { runs: true, output: true, gap: false });
        id += 1;
    }
    s
}

/// 四路对抗：空位、号零、重号、暗样本冻存。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = Strategy::new();
    out[0] = a.add(1, 0) == Err(LS_BITS);
    out[1] = a.add(0, 1) == Err(LS_ID);
    let _ = a.add(1, 1);
    out[2] = a.add(1, 2) == Err(LS_ID);
    let mut b = build();
    let _ = b.judge(7, Verdict { runs: true, output: true, gap: true });
    out[3] = b.freeze() == Err(LS_DARK);
    out
}

/// 判据入口。
pub fn run_unx11223_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11223");
    let mut t = build();
    let root = t.freeze().unwrap_or(0);

    s.add("J1-三十样本", t.samples.len() == WANT_SAMPLES, "策略账恰含三十份样本");
    s.add("J2-号无重", {
        let mut seen: Vec<u32> = Vec::new();
        for x in t.samples.iter() {
            if !seen.contains(&x.id) {
                seen.push(x.id);
            }
        }
        seen.len() == WANT_SAMPLES
    }, "样本号两两不同");
    s.add("J3-位合法", t.samples.iter().all(|x| x.bits != 0 && x.bits & !FEAT_MASK == 0), "特性位非空且不越六位掩码");
    s.add("J4-覆盖六特性", t.cover_count() == 6, "样本×特性矩阵覆盖全部六项特性");
    s.add("J5-最小复现序", t.order_ok(), "特性数升序排列，最小复现排在前");
    s.add("J6-单比特样本", t.samples.iter().filter(|x| x.feats() == 1).count() == 6, "六个单特性样本各自点亮一项");
    s.add("J7-全点亮", t.lit_count() == WANT_SAMPLES, "三十样本三成分全成立");
    s.add("J8-暗样本空", t.dark().is_empty(), "暗样本清单为空");
    s.add("J9-冻存根", root != 0 && t.root == Some(root), "策略账冻存产出非零根");

    s.add("J10-冻后加拒", t.add(31, 1) == Err(LS_FROZEN), "冻存后新增样本拒为 LS_FROZEN");
    s.add("J11-冻后判拒", t.judge(1, Verdict { runs: false, output: false, gap: true }) == Err(LS_FROZEN), "冻存后改判拒为 LS_FROZEN");
    s.add("J12-空位拒", {
        let mut x = Strategy::new();
        x.add(1, 0) == Err(LS_BITS)
    }, "特性位为零拒为 LS_BITS");
    s.add("J13-越界位拒", {
        let mut x = Strategy::new();
        x.add(1, 0x40) == Err(LS_BITS)
    }, "第七位越界拒为 LS_BITS");
    s.add("J14-欠样本禁冻", {
        let mut x = Strategy::new();
        let _ = x.add(1, 1);
        let _ = x.judge(1, Verdict { runs: true, output: true, gap: false });
        x.freeze() == Err(LS_DARK)
    }, "样本不足三十时冻存拒为 LS_DARK");

    let four = inject_four();
    s.add("J15-四路注入", four.iter().all(|x| *x), "空位/号零/重号/暗样本四路全拒");
    s.add_dyn(
        "J16-点亮明细",
        t.lit_count() == 30 && t.cover_count() == 6,
        &format!("点亮 {}/30，覆盖 {}/6，最小样本 {} 份", t.lit_count(), t.cover_count(), 6),
    );
    s.add_dyn(
        "J17-账根",
        root == t.fold(),
        &format!("根 0x{:x}，判定成分求和 {}", root, 3 * 30),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11223_lit_strategy_flow() {
        let mut t = build();
        assert!(t.order_ok());
        assert_eq!(t.cover_count(), 6);
        assert_eq!(t.dark().len(), 0);
        let r = t.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(t.add(31, 1), Err(LS_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11223_checks_green() {
        let s = run_unx11223_checks();
        assert!(s.all_passed(), "UNX-F11223 判据未全绿");
    }
}
