//! UNX-F11208 vvar/vdso 布局满足度登记（0x1f808 段）
//!
//! 锚点：UNX-F11208-J1 vvar 页布局与 glibc clock_gettime 消费路径满足度
//! 落账，vDSO 缺席回退 fast syscall 路径有账。
//! 域：CoRun·UNX-C5-B01——vvar 四页（时钟/校时/时间/CPU）与 vdso 两页按
//! 页序登记，偏移不重叠；消费路径按 vdso 在场与否二分，缺席一律记回退账。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f808;
pub const VV_ALIGN: u32 = SEG_BASE + 0x01; // 偏移未页对齐
pub const VV_OVERLAP: u32 = SEG_BASE + 0x02; // 页区重叠
pub const VV_ROLE: u32 = SEG_BASE + 0x03; // 页角色越界
pub const VV_DUP: u32 = SEG_BASE + 0x04; // 页序号重录
pub const VV_NOFALL: u32 = SEG_BASE + 0x05; // 缺席却无回退账
pub const VV_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后写布局
pub const VV_BADCLK: u32 = SEG_BASE + 0x07; // 时钟号越界

pub const VVAR_PAGES: usize = 4;
pub const VDSO_PAGES: usize = 2;
pub const PAGE: u32 = 4096;
/// vvar 页角色：0 时钟 1 校时 2 时间 3 CPU。
pub const ROLES: usize = 4;
/// 时钟号：0 REALTIME 1 MONO 2 BOOTTIME 3 CPUTIME。
pub const CLOCKS: usize = 4;
pub const VV_TAG: u64 = 0x1f808c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 单页登记：页序号、角色、偏移、可写位。
#[derive(Clone, Debug)]
pub struct Page {
    pub idx: u32,
    pub role: u32,
    pub off: u32,
    pub writable: bool,
}

impl Page {
    pub fn end(&self) -> u32 {
        self.off.saturating_add(PAGE)
    }

    pub fn render(&self) -> String {
        format!("p{}:r{}@{:#x}", self.idx, self.role, self.off)
    }
}

/// vvar/vdso 布局账。
#[derive(Clone, Debug)]
pub struct Layout {
    pub vvar: Vec<Page>,
    pub vdso: Vec<Page>,
    pub present: bool,
    pub syscall_fallbacks: u32,
    pub vdso_hits: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Layout {
    pub fn new(present: bool) -> Layout {
        Layout {
            vvar: Vec::new(),
            vdso: Vec::new(),
            present,
            syscall_fallbacks: 0,
            vdso_hits: 0,
            frozen: false,
            root: None,
        }
    }

    /// 登记 vvar 页：对齐/重叠/角色/重号四路拒。
    pub fn add_vvar(&mut self, idx: u32, role: u32, off: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(VV_FROZEN);
        }
        if off % PAGE != 0 {
            return Err(VV_ALIGN);
        }
        if role as usize >= ROLES {
            return Err(VV_ROLE);
        }
        if self.vvar.iter().any(|p| p.idx == idx || p.off == off) {
            return Err(VV_DUP);
        }
        let page = Page { idx, role, off, writable: false };
        if self.vvar.iter().any(|p| p.off < page.end() && page.off < p.end()) {
            return Err(VV_OVERLAP);
        }
        self.vvar.push(page);
        Ok(())
    }

    /// 登记 vdso 页（只读可执行面）。
    pub fn add_vdso(&mut self, idx: u32, off: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(VV_FROZEN);
        }
        if off % PAGE != 0 {
            return Err(VV_ALIGN);
        }
        if self.vdso.iter().any(|p| p.idx == idx || p.off == off) {
            return Err(VV_DUP);
        }
        let page = Page { idx, role: ROLES as u32, off, writable: false };
        if self.vdso.iter().any(|p| p.off < page.end() && page.off < p.end()) {
            return Err(VV_OVERLAP);
        }
        self.vdso.push(page);
        Ok(())
    }

    /// 布局满足：vvar 四页、vdso 两页、偏移升序不重叠。
    pub fn satisfied(&self) -> bool {
        if self.vvar.len() != VVAR_PAGES || self.vdso.len() != VDSO_PAGES {
            return false;
        }
        let mut k = 0usize;
        while k < self.vvar.len() {
            if self.vvar[k].writable {
                return false;
            }
            if k > 0 && self.vvar[k - 1].off >= self.vvar[k].off {
                return false;
            }
            k += 1;
        }
        k = 0;
        while k < self.vdso.len() {
            if self.vdso[k].writable {
                return false;
            }
            if k > 0 && self.vdso[k - 1].off >= self.vdso[k].off {
                return false;
            }
            k += 1;
        }
        // vdso 与 vvar 页区不得重叠。
        let mut i = 0usize;
        while i < self.vvar.len() {
            let mut j = 0usize;
            while j < self.vdso.len() {
                if self.vvar[i].off < self.vdso[j].end() && self.vdso[j].off < self.vvar[i].end() {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 时钟消费：vdso 在场走 vdso，缺席记 syscall 回退；时钟号越界拒。
    pub fn clock_path(&mut self, clock: u32) -> Result<u32, u32> {
        if clock as usize >= CLOCKS {
            return Err(VV_BADCLK);
        }
        if self.present {
            self.vdso_hits = self.vdso_hits.wrapping_add(1);
            Ok(0)
        } else {
            self.syscall_fallbacks = self.syscall_fallbacks.wrapping_add(1);
            Ok(1)
        }
    }

    /// 回退账成立：缺席必有回退计数且零 vdso 命中。
    pub fn fallback_accounted(&self) -> bool {
        if self.present {
            self.syscall_fallbacks == 0
        } else {
            self.syscall_fallbacks > 0 && self.vdso_hits == 0
        }
    }

    pub fn role_of(&self, clock: u32) -> Option<u32> {
        let mut k = 0usize;
        while k < self.vvar.len() {
            if self.vvar[k].role == clock {
                return Some(self.vvar[k].off);
            }
            k += 1;
        }
        None
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = VV_TAG;
        let mut k = 0usize;
        while k < self.vvar.len() {
            h = (h ^ self.vvar[k].idx as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.vvar[k].role as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.vvar[k].off as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        k = 0;
        while k < self.vdso.len() {
            h = (h ^ self.vdso[k].off as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.present as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.syscall_fallbacks as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.vdso_hits as u64).wrapping_mul(FP_STEP);
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 建布局：vvar 4 页 @0x1000 起，vdso 2 页 @0x5000 起，页大小 4K。
pub fn build_layout(present: bool) -> (Layout, bool) {
    let mut l = Layout::new(present);
    let mut k = 0u32;
    let mut ok = true;
    while (k as usize) < VVAR_PAGES {
        if l.add_vvar(k, k, 0x1000 + k * PAGE).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while (k as usize) < VDSO_PAGES {
        if l.add_vdso(k, 0x5000 + k * PAGE).is_err() {
            ok = false;
        }
        k += 1;
    }
    let good = ok && l.satisfied();
    (l, good)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 未页对齐必拒。
    let mut a = Layout::new(true);
    got[0] = a.add_vvar(0, 0, 0x1001).err() == Some(VV_ALIGN);
    // 2) 页角色越界必拒。
    let mut b = Layout::new(true);
    got[1] = b.add_vvar(0, ROLES as u32, 0x1000).err() == Some(VV_ROLE);
    // 3) 页号重录必拒。
    let mut c = Layout::new(true);
    let _ = c.add_vvar(0, 0, 0x1000);
    got[2] = c.add_vvar(0, 1, 0x2000).err() == Some(VV_DUP);
    // 4) 缺席无回退账不成立；记一次回退后成立。
    let mut d = Layout::new(false);
    let before = d.fallback_accounted();
    let _ = d.clock_path(1);
    got[3] = !before && d.fallback_accounted() && d.syscall_fallbacks == 1;
    got
}

/// 批内判据。
pub fn run_unx11208_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11208");
    let (mut l, built) = build_layout(true);

    s.add("J1-布局满足", built && l.satisfied(), "vvar 四页 + vdso 两页布局成立");
    s.add("J2-页数定额", l.vvar.len() == VVAR_PAGES && l.vdso.len() == VDSO_PAGES,
        "vvar 四页与 vdso 两页页数定额");
    s.add("J3-页对齐", l.vvar.iter().all(|p| p.off % PAGE == 0) && l.vdso.iter().all(|p| p.off % PAGE == 0),
        "全部页偏移 4K 对齐");
    s.add("J4-只读面", l.vvar.iter().all(|p| !p.writable) && l.vdso.iter().all(|p| !p.writable),
        "vvar 与 vdso 均不可写");
    s.add("J5-时钟角色", l.role_of(0) == Some(0x1000) && l.role_of(3) == Some(0x4000),
        "时钟与 CPU 页角色可定位");

    let mut hits = 0u32;
    let mut k = 0u32;
    while (k as usize) < CLOCKS {
        if l.clock_path(k) == Ok(0) {
            hits += 1;
        }
        k += 1;
    }
    s.add("J6-在场走 vdso", hits == CLOCKS as u32 && l.vdso_hits == CLOCKS as u32,
        "vdso 在场四个时钟全走快路径");

    let (mut off, _) = build_layout(false);
    let _ = off.clock_path(0);
    let _ = off.clock_path(2);
    s.add("J7-缺席回退账", off.syscall_fallbacks == 2 && off.fallback_accounted(),
        "vdso 缺席两次时钟全记 syscall 回退");

    s.add("J8-在场零回退", l.syscall_fallbacks == 0 && l.fallback_accounted(),
        "vdso 在场回退计数为零");

    s.add("J9-坏时钟号拒", off.clock_path(CLOCKS as u32).err() == Some(VV_BADCLK),
        "时钟号越界必拒");

    let inj = inject_four();
    s.add("J10-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "未对齐/坏角色/重页号/无回退账四路必抓");

    let root = l.freeze();
    s.add("J11-冻结根", l.fold() == root && root != 0, "冻结后布局指纹可重算");
    s.add("J12-冻结拒写", l.add_vvar(9, 0, 0x9000).err() == Some(VV_FROZEN),
        "冻结后布局再登记必拒");

    let fp = l.fold();
    s.add_dyn("J13-布局明细", l.satisfied() && fp == root,
        &format!("vvar={} vdso={}", l.vvar.len(), l.vdso.len()));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11208_vdso_flow() {
        let (mut l, built) = build_layout(true);
        assert!(built);
        assert!(l.satisfied());
        assert_eq!(l.clock_path(0), Ok(0));
        assert_eq!(l.clock_path(9), Err(VV_BADCLK));
        let (off, _) = build_layout(false);
        let mut o = off;
        assert_eq!(o.clock_path(1), Ok(1));
        assert!(o.fallback_accounted());
        let root = l.freeze();
        assert_eq!(l.fold(), root);
        assert!(l.add_vdso(7, 0x9000).is_err());
    }

    #[test]
    fn unx11208_checks_green() {
        let s = run_unx11208_checks();
        assert!(s.all_passed(), "UNX-F11208 判据未全绿");
    }
}
