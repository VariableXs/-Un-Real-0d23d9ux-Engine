//! UNX-F11224 匿名 mmap 提示行为消费账（0x1f824 段）
//!
//! 锚点：UNX-F11224-J1 glibc mmap 提示地址使用态与内核 hint 遵守语义对账，
//! hint 冲突回退路径判据绿。
//! 域：CoRun·UNX-C5-B02——hint 只是「建议」：不冲突就照给，冲突就顺延，
//! 给零就由内核挑；三种形态都要在账上留下 got 与 fallback 两列。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f824;
pub const MH_LEN: u32 = SEG_BASE + 0x01; // 长度为零
pub const MH_ALIGN: u32 = SEG_BASE + 0x02; // hint 未页对齐
pub const MH_ORDER: u32 = SEG_BASE + 0x03; // 提示序倒挂
pub const MH_MISS: u32 = SEG_BASE + 0x04; // 账上无冲突回退样本
pub const MH_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后改写
pub const MH_OVER: u32 = SEG_BASE + 0x06; // 超容
pub const MH_BAD: u32 = SEG_BASE + 0x07; // 冲突地址未被顺延

/// 页大小与请求上限。
pub const PAGE: u64 = 4096;
pub const WANT: usize = 30;
pub const CAP: usize = 48;
pub const MH_TAG: u64 = 0x1f824b9;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一次匿名映射请求。
#[derive(Clone, Debug)]
pub struct Request {
    pub id: u32,
    pub hint: u64,
    pub len: u64,
    pub got: u64,
    pub fallback: bool,
}

impl Request {
    pub fn end(&self) -> u64 {
        self.got.saturating_add(self.len)
    }

    pub fn honored(&self) -> bool {
        self.hint != 0 && self.got == self.hint
    }

    pub fn rounded(&self, page: u64) -> bool {
        self.len != 0 && self.len % page == 0
    }
}

/// 提示消费账。
#[derive(Clone, Debug)]
pub struct Mapper {
    pub reqs: Vec<Request>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Mapper {
    pub fn new() -> Mapper {
        Mapper { reqs: Vec::new(), frozen: false, root: None }
    }

    pub fn round_pages(len: u64) -> u64 {
        if len == 0 {
            return 0;
        }
        len.saturating_add(PAGE - 1) / PAGE * PAGE
    }

    fn occupied(&self, start: u64, len: u64) -> bool {
        let mut k = 0usize;
        while k < self.reqs.len() {
            let r = &self.reqs[k];
            if start < r.end() && r.got < start.saturating_add(len) {
                return true;
            }
            k += 1;
        }
        false
    }

    fn next_free(&self, hint: u64, len: u64) -> u64 {
        let mut a = hint.saturating_add(PAGE);
        let mut n = 0u32;
        while n < 256 {
            if !self.occupied(a, len) {
                return a;
            }
            a = a.saturating_add(PAGE);
            n += 1;
        }
        a
    }

    /// 申请：零长、未对齐两路拒；冲突顺延。
    pub fn request(&mut self, hint: u64, len: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(MH_FROZEN);
        }
        if len == 0 {
            return Err(MH_LEN);
        }
        if hint != 0 && hint % PAGE != 0 {
            return Err(MH_ALIGN);
        }
        if self.reqs.len() >= CAP {
            return Err(MH_OVER);
        }
        let n = Self::round_pages(len);
        let got = if hint == 0 {
            let mut a = 0x7f00_0000_0000u64;
            let mut k = 0usize;
            while k < self.reqs.len() {
                a = a.saturating_add(self.reqs[k].len);
                k += 1;
            }
            a
        } else if self.occupied(hint, n) {
            self.next_free(hint, n)
        } else {
            hint
        };
        let id = self.reqs.len() as u32 + 1;
        self.reqs.push(Request { id, hint, len: n, got, fallback: hint != 0 && got != hint });
        Ok(got)
    }

    pub fn honored(&self) -> usize {
        self.reqs.iter().filter(|r| r.honored()).count()
    }

    pub fn fallbacks(&self) -> usize {
        self.reqs.iter().filter(|r| r.fallback).count()
    }

    pub fn kernel_picks(&self) -> usize {
        self.reqs.iter().filter(|r| r.hint == 0).count()
    }

    pub fn disjoint(&self) -> bool {
        let mut i = 0usize;
        while i < self.reqs.len() {
            let mut j = i + 1;
            while j < self.reqs.len() {
                let a = &self.reqs[i];
                let b = &self.reqs[j];
                if a.got < b.end() && b.got < a.end() {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(MH_FROZEN);
        }
        if self.reqs.len() != WANT {
            return Err(MH_ORDER);
        }
        if self.fallbacks() == 0 {
            return Err(MH_MISS);
        }
        if !self.disjoint() {
            return Err(MH_BAD);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("hint {}/{} fallback {}", self.honored(), self.reqs.len(), self.fallbacks())
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = MH_TAG;
        let mut k = 0usize;
        while k < self.reqs.len() {
            h = (h ^ self.reqs[k].hint).wrapping_mul(FP_STEP);
            h = (h ^ self.reqs[k].got).wrapping_mul(FP_STEP);
            h = (h ^ self.reqs[k].len).wrapping_mul(FP_STEP);
            h = (h ^ self.reqs[k].fallback as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// glibc 提示地址基（页对齐）。
pub const HINT_BASE: u64 = 0x7f10_0000_0000;

/// 组一张三十次请求、含内核挑选与冲突回退的账。
pub fn build() -> Mapper {
    let mut m = Mapper::new();
    let mut i = 1u64;
    while i <= WANT as u64 {
        if i % 10 == 0 {
            let _ = m.request(0, PAGE);
        } else if i % 10 == 5 {
            let _ = m.request(HINT_BASE + 3 * 0x2000, PAGE);
        } else {
            let _ = m.request(HINT_BASE + i * 0x2000, PAGE);
        }
        i += 1;
    }
    m
}

/// 四路对抗：零长、未对齐、欠样本冻存、冻后申请。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = Mapper::new();
    out[0] = a.request(HINT_BASE, 0) == Err(MH_LEN);
    out[1] = a.request(HINT_BASE + 1, PAGE) == Err(MH_ALIGN);
    let mut b = Mapper::new();
    let _ = b.request(HINT_BASE, PAGE);
    out[2] = b.freeze() == Err(MH_ORDER);
    let mut c = build();
    let _ = c.freeze();
    out[3] = c.request(HINT_BASE, PAGE) == Err(MH_FROZEN);
    out
}

/// 判据入口。
pub fn run_unx11224_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11224");
    let mut m = build();
    let root = m.freeze().unwrap_or(0);

    s.add("J1-三十请求", m.reqs.len() == WANT, "匿名映射请求恰三十次");
    s.add("J2-长度页整", m.reqs.iter().all(|r| r.rounded(PAGE)), "每次请求长度按页上取整");
    s.add("J3-hint 对齐", m.reqs.iter().all(|r| r.hint == 0 || r.hint % PAGE == 0), "非零 hint 全部页对齐");
    s.add("J4-内核挑址", m.kernel_picks() == 3, "三次零 hint 交由内核挑址");
    s.add("J5-冲突回退", m.fallbacks() == 3, "三次冲突 hint 走回退路径");
    s.add("J6-顺延方向", m.reqs.iter().filter(|r| r.fallback).all(|r| r.got > r.hint), "回退地址一律位于 hint 之后");
    s.add("J7-命中二十四", m.honored() == 24, "无冲突 hint 二十四次原样命中");
    s.add("J8-区间不重叠", m.disjoint(), "三十段映射两两不重叠");
    s.add("J9-冻存根", root != 0 && m.root == Some(root), "提示账冻存产出非零根");

    s.add("J10-冻后申请拒", m.request(HINT_BASE, PAGE) == Err(MH_FROZEN), "冻存后申请拒为 MH_FROZEN");
    s.add("J11-零长拒", {
        let mut x = Mapper::new();
        x.request(0, 0) == Err(MH_LEN)
    }, "零长请求拒为 MH_LEN");
    s.add("J12-未对齐拒", {
        let mut x = Mapper::new();
        x.request(HINT_BASE + 0x801, PAGE) == Err(MH_ALIGN)
    }, "半页 hint 拒为 MH_ALIGN");

    let mut conflict = Mapper::new();
    let _ = conflict.request(HINT_BASE, PAGE);
    let g = conflict.request(HINT_BASE, PAGE).unwrap_or(0);
    s.add("J13-冲突顺延", g == HINT_BASE + PAGE && conflict.fallbacks() == 1, "同址二次请求顺延一页并记回退");

    let four = inject_four();
    s.add("J14-四路注入", four.iter().all(|x| *x), "零长/未对齐/欠样本/冻后申请四路全拒");
    s.add_dyn(
        "J15-回退明细",
        m.honored() + m.fallbacks() + m.kernel_picks() == 30,
        &format!("命中 {}，回退 {}，内核挑 {}，总 {}", m.honored(), m.fallbacks(), m.kernel_picks(), m.reqs.len()),
    );
    s.add_dyn(
        "J16-账根",
        root == m.fold(),
        &format!("根 0x{:x}，末段 [0x{:x},0x{:x})", root, m.reqs[29].got, m.reqs[29].end()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11224_mmap_hint_flow() {
        let mut m = build();
        assert_eq!(m.fallbacks(), 3);
        assert!(m.disjoint());
        let r = m.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(m.request(HINT_BASE, PAGE), Err(MH_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11224_checks_green() {
        let s = run_unx11224_checks();
        assert!(s.all_passed(), "UNX-F11224 判据未全绿");
    }
}
