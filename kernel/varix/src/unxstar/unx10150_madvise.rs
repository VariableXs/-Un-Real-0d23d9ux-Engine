//! UNX 卡 10150 — madvise 正反双判据（建议语义达成与非法建议误用受控）
//! 段位 seg=0x105e0
//! 判据原文：UNX-F10150-J1 建议达成（MADV_NORMAL/DONTNEED 语义 LTP 面 10/10 次）
//! 锚点：MADV 五档建议落账 + DONTNEED 清驻留 + 非法建议受控
//! 域：AdvBook 建议册（区间 × 建议档 × 驻留位）
//! 结构：区间挂账 → advise 落档 → DONTNEED 弃驻留 → 非法档受控 → 审计与注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x105e0;
pub const AD_BAD_ADV: u32 = SEG_BASE + 0x01;
pub const AD_UNMAPPED: u32 = SEG_BASE + 0x02;
pub const AD_SEALED: u32 = SEG_BASE + 0x03;
pub const AD_ZERO_LEN: u32 = SEG_BASE + 0x04;
pub const AD_CAP: u32 = SEG_BASE + 0x05;

/// 建议档闭集
pub const V_NORMAL: u32 = 0;
pub const V_SEQUENTIAL: u32 = 1;
pub const V_RANDOM: u32 = 2;
pub const V_DONTNEED: u32 = 3;
pub const V_DONTFORK: u32 = 4;

/// 区间容量
pub const REGION_CAP: usize = 0x40;

/// 一个被建议区间
#[derive(Clone, Copy, Debug)]
pub struct AdvRegion {
    pub start: usize,
    pub len: usize,
    pub adv: u32,
    pub resident: bool,
}

/// 建议册
pub struct AdvBook {
    pub regions: Vec<AdvRegion>,
    sealed: bool,
}

impl AdvBook {
    pub fn new() -> Self {
        AdvBook {
            regions: Vec::new(),
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂区间（模拟既有 mmap 区）
    pub fn attach(&mut self, start: usize, len: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(AD_SEALED);
        }
        if len == 0 {
            return Err(AD_ZERO_LEN);
        }
        if self.regions.len() >= REGION_CAP {
            return Err(AD_CAP);
        }
        self.regions.push(AdvRegion {
            start,
            len,
            adv: V_NORMAL,
            resident: true,
        });
        Ok(self.regions.len() - 1)
    }

    /// madvise 主语义：落建议档；DONTNEED 清驻留
    pub fn advise(&mut self, idx: usize, adv: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(AD_SEALED);
        }
        let r = self.regions.get_mut(idx).ok_or(AD_UNMAPPED)?;
        if adv > V_DONTFORK {
            return Err(AD_BAD_ADV);
        }
        let old = r.adv;
        r.adv = adv;
        if adv == V_DONTNEED {
            r.resident = false;
        } else if old == V_DONTNEED {
            r.resident = true;
        }
        Ok(old)
    }

    /// 档位查询
    pub fn adv_of(&self, idx: usize) -> Result<u32, u32> {
        self.regions.get(idx).map(|r| r.adv).ok_or(AD_UNMAPPED)
    }

    /// 驻留位查询
    pub fn resident_of(&self, idx: usize) -> Result<bool, u32> {
        self.regions.get(idx).map(|r| r.resident).ok_or(AD_UNMAPPED)
    }

    /// DONTNEED 语义核：弃驻留后重访不再命中
    pub fn dontneed_probe(&self, idx: usize) -> bool {
        match self.regions.get(idx) {
            Some(r) => r.adv == V_DONTNEED && !r.resident,
            None => false,
        }
    }

    /// 档分布
    pub fn adv_hist(&self) -> [usize; 5] {
        let mut h = [0; 5];
        for r in &self.regions {
            if (r.adv as usize) < 5 {
                h[r.adv as usize] += 1;
            }
        }
        h
    }

    /// 非默认建议数
    pub fn advised_count(&self) -> usize {
        self.regions.iter().filter(|r| r.adv != V_NORMAL).count()
    }

    /// 指纹
    pub fn render(&self) -> String {
        let mut s = String::new();
        for r in &self.regions {
            s.push_str(&format!("{}:{}{};", r.start, r.adv, if r.resident { "R" } else { "n" }));
        }
        s
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 区间数
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// 驻留区间数
    pub fn resident_count(&self) -> usize {
        self.regions.iter().filter(|r| r.resident).count()
    }

    /// 指定档在册计数
    pub fn adv_probe(&self, adv: u32) -> usize {
        self.regions.iter().filter(|r| r.adv == adv).count()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = AdvBook::new();
    let i = b.attach(0x1000, 0x2000).unwrap_or(0);
    // 1) 非法建议档必拒
    got[0] = b.advise(i, 9).is_err();
    // 2) 零长挂区间必拒
    got[1] = b.attach(0x8000, 0).is_err();
    // 3) 野区间柄必拒
    got[2] = b.advise(99, V_NORMAL).is_err();
    // 4) DONTNEED 弃驻留必生效
    let _ = b.advise(i, V_DONTNEED);
    got[3] = !b.resident_of(i).unwrap_or(true);
    // 5) 封册后必拒
    b.seal();
    got[4] = b.advise(i, V_NORMAL).is_err();
    got
}

/// 判据入口：UNX-F10150
pub fn run_unx10150_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10150");
    let mut b = AdvBook::new();

    // J1 建议达成 10/10：十区间逐档落建议
    let mut j1 = true;
    for i in 0..10usize {
        let idx = b.attach(0x1000 * (i + 1), 0x1000);
        let idx = match idx {
            Ok(v) => v,
            Err(_) => {
                j1 = false;
                break;
            }
        };
        let adv = (i % 5) as u32;
        if b.advise(idx, adv) != Ok(V_NORMAL) {
            j1 = false;
        }
        if b.adv_of(idx) != Ok(adv) {
            j1 = false;
        }
    }
    s.add("J1_advise_10x", j1, "十区间五档建议落账 10/10");

    // J2 DONTNEED 弃驻留
    let j2 = b.dontneed_probe(3) && b.resident_of(3) == Ok(false);
    s.add("J2_dontneed", j2, "DONTNEED 清驻留位");

    // J3 弃驻留后复活
    let r = b.advise(3, V_SEQUENTIAL);
    let j3 = r == Ok(V_DONTNEED) && b.resident_of(3) == Ok(true);
    s.add("J3_revive", j3, "重建议自 DONTNEED 复活驻留");

    // J4 非法档受控
    let j4 = b.advise(0, 5) == Err(AD_BAD_ADV)
        && b.advise(0, 0xFF) == Err(AD_BAD_ADV)
        && b.adv_of(0) == Ok(V_NORMAL);
    s.add("J4_bad_adv", j4, "越闭集档位双拒且原档不动");

    // J5 野柄受控
    let j5 = b.advise(99, V_NORMAL) == Err(AD_UNMAPPED)
        && b.adv_of(99) == Err(AD_UNMAPPED)
        && b.resident_of(99) == Err(AD_UNMAPPED);
    s.add("J5_bad_idx", j5, "野柄三路查询受控 UNMAPPED");

    // J6 零长受控
    s.add("J6_zero_len", b.attach(0x90000, 0) == Err(AD_ZERO_LEN), "零长区间挂账受控");

    // J7 档分布直方
    let h = b.adv_hist();
    let sum: usize = h.iter().sum();
    s.add("J7_hist", sum == 10 && h[V_SEQUENTIAL as usize] == 3 && h[V_DONTNEED as usize] == 1, "五档直方 10 区间对平（J3 已改档 3→SEQ）");

    // J8 非默认建议计数
    let j8 = b.advised_count() == 8;
    s.add("J8_advised", j8, "非默认档 8 区间（i=0/5 回绕同档保持 NORMAL）");

    // J9 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J9_inject_all", caught == 5, "5 注入点全检出");

    // J10 封册受控
    let mut b2 = AdvBook::new();
    let i2 = b2.attach(0x1000, 0x1000).unwrap_or(0);
    b2.seal();
    let j10 = b2.advise(i2, V_NORMAL) == Err(AD_SEALED)
        && b2.attach(0x2000, 0x1000) == Err(AD_SEALED);
    s.add("J10_sealed", j10, "封册后建议/挂账皆拒 SEALED");

    // J11 指纹稳定
    let f1 = b.render();
    let f2 = b.render();
    s.add("J11_fingerprint", f1 == f2 && f1.contains(":3n;"), "档位驻留指纹稳定");

    // J12 区间容量守门
    let mut b3 = AdvBook::new();
    let mut fill_ok = true;
    for i in 0..REGION_CAP {
        if b3.attach(i * 0x1000, 0x1000).is_err() {
            fill_ok = false;
        }
    }
    let j12 = fill_ok && b3.attach(0, 0x1000) == Err(AD_CAP);
    s.add("J12_cap", j12, "区间满 64 拒 CAP");

    // J13 旧档返回
    let mut b4 = AdvBook::new();
    let i4 = b4.attach(0x1000, 0x1000).unwrap_or(0);
    let _ = b4.advise(i4, V_RANDOM);
    let old = b4.advise(i4, V_NORMAL);
    s.add("J13_old_adv", old == Ok(V_RANDOM), "改档返旧档");

    // J14 NORMAL 缺省
    let mut b5 = AdvBook::new();
    let i5 = b5.attach(0x1000, 0x1000).unwrap_or(0);
    s.add("J14_default", b5.adv_of(i5) == Ok(V_NORMAL) && b5.resident_of(i5) == Ok(true), "缺省 NORMAL 且驻留");

    // J15 DONTFORK 语义面
    let _ = b5.advise(i5, V_DONTFORK);
    s.add("J15_dontfork", b5.adv_of(i5) == Ok(V_DONTFORK) && b5.resident_of(i5) == Ok(true), "DONTFORK 落档且驻留不动");

    // J16 区间计数
    let j16 = b.region_count() == 10;
    s.add("J16_regions", j16, "十区间在册");

    // J17 驻留计数
    let j17 = b.resident_count() == 9;
    s.add("J17_resident", j17, "弃驻留 1 区余 9 区驻留");

    // J18 建议账守恒
    let sum: usize = b.adv_hist().iter().sum();
    s.add("J18_hist_sum", sum == b.region_count(), "档直方和与区间数等");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adv50_flow() {
        let mut b = AdvBook::new();
        let i = b.attach(0x1000, 0x1000).unwrap();
        assert_eq!(b.advise(i, V_DONTNEED), Ok(V_NORMAL));
        assert_eq!(b.resident_of(i), Ok(false));
        assert_eq!(b.advise(i, V_RANDOM), Ok(V_DONTNEED));
        assert_eq!(b.resident_of(i), Ok(true));
    }

    #[test]
    fn test_adv50_misuse() {
        let mut b = AdvBook::new();
        let i = b.attach(0x1000, 0x1000).unwrap();
        assert_eq!(b.advise(i, 9), Err(AD_BAD_ADV));
        assert_eq!(b.advise(9, V_NORMAL), Err(AD_UNMAPPED));
        assert_eq!(b.attach(0, 0), Err(AD_ZERO_LEN));
    }

    #[test]
    fn test_adv50_checks() {
        let cs = run_unx10150_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_adv50_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
