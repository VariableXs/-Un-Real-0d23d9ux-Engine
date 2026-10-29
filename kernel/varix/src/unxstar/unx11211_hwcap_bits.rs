//! UNX-F11211 AT_HWCAP 位语义满足度（0x1f811 段）
//!
//! 锚点：UNX-F11211-J1 HWCAP 位清单与 glibc ifunc 选择器消费位逐位对账，
//! 未实现位的 ifunc 回退路径有账。
//! 域：CoRun·UNX-C5-B01——HWCAP 以位掩码建模，选择器声明所需位；需求位
//! 全在掩码内走快实现，否则落通用实现，两者都计数，未消费位显性列账。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f811;
pub const HW_BIT: u32 = SEG_BASE + 0x01; // 位号越界
pub const HW_NAME: u32 = SEG_BASE + 0x02; // 位名空
pub const HW_DUP: u32 = SEG_BASE + 0x03; // 位号重录
pub const HW_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写
pub const HW_NOMASK: u32 = SEG_BASE + 0x05; // 选择器需求位为空
pub const HW_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 位清单容量。
pub const BITS: usize = 8;
pub const HW_TAG: u64 = 0x1f811c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// HWCAP 位定义：位号、名、实现态。
#[derive(Clone, Debug)]
pub struct Bit {
    pub no: u32,
    pub name: &'static str,
    pub implemented: bool,
}

impl Bit {
    pub fn mask(&self) -> u32 {
        if self.no < 32 {
            1u32 << self.no
        } else {
            0
        }
    }

    pub fn render(&self) -> String {
        format!("{}:{}:{}", self.name, self.no, if self.implemented { "impl" } else { "soft" })
    }
}

/// HWCAP 位清单（8 位，其中 ERMS 未实现作为回退样本）。
#[derive(Clone, Debug)]
pub struct Hwcap {
    pub bits: Vec<Bit>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Hwcap {
    pub fn new() -> Hwcap {
        Hwcap { bits: Vec::new(), frozen: false, root: None }
    }

    pub fn add(&mut self, no: u32, name: &'static str, implemented: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(HW_FROZEN);
        }
        if no as usize >= BITS {
            return Err(HW_BIT);
        }
        if name.is_empty() {
            return Err(HW_NAME);
        }
        if self.bits.iter().any(|b| b.no == no) {
            return Err(HW_DUP);
        }
        if self.bits.len() >= BITS {
            return Err(HW_OVER);
        }
        self.bits.push(Bit { no, name, implemented });
        Ok(())
    }

    /// 运行期掩码：只有已实现位可置。
    pub fn mask(&self) -> u32 {
        let mut m = 0u32;
        let mut k = 0usize;
        while k < self.bits.len() {
            if self.bits[k].implemented {
                m |= self.bits[k].mask();
            }
            k += 1;
        }
        m
    }

    /// 未实现位名清单。
    pub fn unimplemented(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut k = 0usize;
        while k < self.bits.len() {
            if !self.bits[k].implemented {
                out.push(self.bits[k].name);
            }
            k += 1;
        }
        out
    }

    pub fn has(&self, name: &str) -> bool {
        self.bits.iter().any(|b| b.name == name)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = HW_TAG;
        let mut k = 0usize;
        while k < self.bits.len() {
            h = (h ^ self.bits[k].no as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.bits[k].implemented as u64).wrapping_mul(FP_STEP);
            let bytes = self.bits[k].name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            k += 1;
        }
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// ifunc 选择器：需求位掩码 + 快/通用两实现名。
#[derive(Clone, Debug)]
pub struct Selector {
    pub name: &'static str,
    pub need: u32,
    pub fast: &'static str,
    pub generic: &'static str,
    pub fast_hits: u32,
    pub slow_hits: u32,
}

impl Selector {
    /// 选择：需求位全在掩码内走快实现，否则通用实现。
    pub fn pick(&mut self, mask: u32) -> &'static str {
        if self.need != 0 && (mask & self.need) == self.need {
            self.fast_hits = self.fast_hits.wrapping_add(1);
            self.fast
        } else {
            self.slow_hits = self.slow_hits.wrapping_add(1);
            self.generic
        }
    }

    pub fn fallback_accounted(&self) -> bool {
        self.slow_hits > 0 || self.need == 0
    }
}

/// 选择器台账。
#[derive(Clone, Debug)]
pub struct IfuncBook {
    pub sels: Vec<Selector>,
}

impl IfuncBook {
    pub fn new() -> IfuncBook {
        IfuncBook { sels: Vec::new() }
    }

    pub fn enroll(&mut self, name: &'static str, need: u32, fast: &'static str, generic: &'static str) -> Result<(), u32> {
        if need == 0 {
            return Err(HW_NOMASK);
        }
        if self.sels.iter().any(|s| s.name == name) {
            return Err(HW_DUP);
        }
        self.sels.push(Selector { name, need, fast, generic, fast_hits: 0, slow_hits: 0 });
        Ok(())
    }

    /// 未消费位：掩码里置位但不被任何选择器需要的位。
    pub fn uncovered(&self, mask: u32) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut b = 0u32;
        while b < BITS as u32 {
            let m = 1u32 << b;
            if mask & m != 0 {
                let mut used = false;
                let mut k = 0usize;
                while k < self.sels.len() {
                    if self.sels[k].need & m != 0 {
                        used = true;
                    }
                    k += 1;
                }
                if !used {
                    out.push(b);
                }
            }
            b += 1;
        }
        out
    }

    pub fn all_picked(&self) -> bool {
        self.sels.iter().all(|s| s.fast_hits + s.slow_hits > 0)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = HW_TAG ^ 0x1f;
        let mut k = 0usize;
        while k < self.sels.len() {
            h = (h ^ self.sels[k].need as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.sels[k].fast_hits as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.sels[k].slow_hits as u64).wrapping_mul(FP_STEP);
            let bytes = self.sels[k].name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            k += 1;
        }
        h
    }
}

/// 建清单：八位（ERMS 未实现）与三个选择器。
pub fn build() -> (Hwcap, IfuncBook, bool) {
    let mut h = Hwcap::new();
    let rows: [(u32, &'static str, bool); 8] = [
        (0, "SSE2", true),
        (1, "AVX", true),
        (2, "AVX2", true),
        (3, "FMA", true),
        (4, "BMI1", true),
        (5, "BMI2", true),
        (6, "ERMS", false),
        (7, "FSGSBASE", true),
    ];
    let mut ok = true;
    let mut k = 0usize;
    while k < rows.len() {
        if h.add(rows[k].0, rows[k].1, rows[k].2).is_err() {
            ok = false;
        }
        k += 1;
    }
    let mut b = IfuncBook::new();
    if b.enroll("memcpy", (1 << 2) | (1 << 3), "memcpy_avx2", "memcpy_generic").is_err() {
        ok = false;
    }
    if b.enroll("strlen", 1 << 2, "strlen_avx2", "strlen_generic").is_err() {
        ok = false;
    }
    if b.enroll("strcmp", 1 << 6, "strcmp_erms", "strcmp_generic").is_err() {
        ok = false;
    }
    (h, b, ok)
}

/// 跑一轮选择：返回 (快命中数, 通用命中数)。
pub fn run_selects(h: &Hwcap, b: &mut IfuncBook) -> (u32, u32) {
    let mask = h.mask();
    let mut fast = 0u32;
    let mut slow = 0u32;
    let mut k = 0usize;
    while k < b.sels.len() {
        let pick = b.sels[k].pick(mask);
        if pick == b.sels[k].fast {
            fast = fast.wrapping_add(1);
        } else {
            slow = slow.wrapping_add(1);
        }
        k += 1;
    }
    (fast, slow)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 位号越界必拒。
    let mut a = Hwcap::new();
    got[0] = a.add(BITS as u32, "X", true).err() == Some(HW_BIT);
    // 2) 需求掩码为空的选择器必拒。
    let mut b = IfuncBook::new();
    got[1] = b.enroll("empty", 0, "f", "g").err() == Some(HW_NOMASK);
    // 3) 未实现位不进运行掩码。
    let (h, _, _) = build();
    got[2] = h.mask() & (1 << 6) == 0 && !h.unimplemented().is_empty();
    // 4) 冻结后加位必拒。
    let (mut f, _, _) = build();
    f.freeze();
    got[3] = f.add(0, "DUP", true).err() == Some(HW_FROZEN);
    got
}

/// 批内判据。
pub fn run_unx11211_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11211");
    let (mut h, mut b, built) = build();

    s.add("J1-八位清单", built && h.bits.len() == BITS, "HWCAP 八位逐位登记");
    s.add("J2-实现配额", h.mask().count_ones() == 7 && h.unimplemented() == vec!["ERMS"],
        "七位实现一位未实现且可枚举");
    s.add("J3-位掩码去未实现", h.mask() & (1 << 6) == 0, "未实现位不置入运行掩码");

    let (fast, slow) = run_selects(&h, &mut b);
    s.add("J4-快实现命中", fast == 2 && b.sels[0].fast_hits == 1 && b.sels[1].fast_hits == 1,
        "需求位齐备的 memcpy/strlen 走快实现");
    s.add("J5-回退有账", slow == 1 && b.sels[2].slow_hits == 1 && b.sels[2].fallback_accounted(),
        "strcmp 因 ERMS 未实现回退通用且入账");
    s.add("J6-未消费位可列", b.uncovered(h.mask()).len() == 5,
        "未被任何选择器消费的三位显性列出");

    s.add("J7-选择器在册", b.sels.len() == 3 && b.all_picked(), "三选择器全部跑过选择");
    s.add("J8-需求子集判", {
        let mut c = Selector { name: "x", need: 0b11, fast: "f", generic: "g", fast_hits: 0, slow_hits: 0 };
        c.pick(0b01) == "g" && c.pick(0b11) == "f"
    }, "需求位部分缺走通用、全齐走快");
    s.add("J9-位名查", h.has("AVX2") && !h.has("AVX512"), "位名查询正反两态");

    let inj = inject_four();
    s.add("J10-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "越界位/空需求/未实现位/冻结四路必抓");

    let root = h.freeze();
    s.add("J11-冻结根", h.fold() == root && root != 0, "冻结后位清单指纹可重算");
    s.add("J12-冻结拒写", h.add(0, "SSE2b", true).err() == Some(HW_FROZEN), "冻结后加位必拒");

    let fp = h.fold();
    let fb = b.sels[2].slow_hits;
    s.add_dyn("J13-选择明细", fast == 2 && slow == 1 && fp == root,
        &format!("fast={} slow={} fb={}", fast, slow, fb));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11211_hwcap_flow() {
        let (mut h, mut b, built) = build();
        assert!(built);
        assert_eq!(h.mask().count_ones(), 7);
        assert_eq!(run_selects(&h, &mut b), (2, 1));
        assert_eq!(b.sels[2].pick(h.mask()), "strcmp_generic");
        assert!(b.sels[2].fallback_accounted());
        assert_eq!(h.uncovered(h.mask()).len(), 3);
        let root = h.freeze();
        assert_eq!(h.fold(), root);
        assert_eq!(h.add(1, "AVX", true), Err(HW_DUP));
    }

    #[test]
    fn unx11211_checks_green() {
        let s = run_unx11211_checks();
        assert!(s.all_passed(), "UNX-F11211 判据未全绿");
    }
}
