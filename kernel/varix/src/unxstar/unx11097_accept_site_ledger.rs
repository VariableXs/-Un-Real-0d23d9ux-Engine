//! UNX-F11097 域验收判据落点对账（IPC 压测/vim 全屏两落点核销）（0x1ec20 段）
//!
//! 锚点：UNX-F11097-J1 落点逐件核销对平——IPC 压测与 vim 全屏两落点的
//! 验收判据逐件核销入账对平；J2 验收判据零悬空——每条判据必落两落点
//! 之一，无悬空判据无漏核销。
//! 域：CoRun·UNX-C4（长稳监控域）。
//!
//! 结构：落点账（判据：编号 + 落点枚举 + 核销位）+ 对账器（两落点核销
//! 计数对平 + 悬空探测）+ 注入核（重判据/未知判据/悬空核销/封账写入
//! 四路必抓）。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x1ec20 独占 0x20）。
pub const SEG_BASE: u32 = 0x1ec20;
pub const LC_DUP: u32 = SEG_BASE + 0x01; // 判据重号
pub const LC_NOENT: u32 = SEG_BASE + 0x02; // 判据不在册
pub const LC_DANGLING: u32 = SEG_BASE + 0x03; // 悬空判据（未落点）
pub const LC_FROZEN: u32 = SEG_BASE + 0x04; // 封账后写账
pub const LC_TWICE: u32 = SEG_BASE + 0x05; // 重复核销

/// 两落点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Site {
    IpcStress,
    VimFull,
}

/// 判据项：编号 + 落点 + 核销位。
#[derive(Clone, Copy, Debug)]
pub struct CritItem {
    pub no: u32,
    pub site: Site,
    pub done: bool,
}

/// 落点对账册。
pub struct SiteBook {
    pub items: Vec<CritItem>,
    pub sealed: bool,
}

impl SiteBook {
    pub fn new() -> SiteBook {
        SiteBook { items: Vec::new(), sealed: false }
    }

    fn at(&self, no: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].no == no {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记判据（必须落点，零落点即悬空必拒）。
    pub fn enroll(&mut self, no: u32, site: Option<Site>) -> Result<(), u32> {
        if self.sealed {
            return Err(LC_FROZEN);
        }
        if self.at(no).is_some() {
            return Err(LC_DUP);
        }
        match site {
            None => Err(LC_DANGLING),
            Some(s) => {
                self.items.push(CritItem { no, site: s, done: false });
                Ok(())
            }
        }
    }

    /// 核销：只有未核销可核销，重复必拒。
    pub fn verify(&mut self, no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(LC_FROZEN);
        }
        match self.at(no) {
            None => Err(LC_NOENT),
            Some(i) => {
                if self.items[i].done {
                    return Err(LC_TWICE);
                }
                self.items[i].done = true;
                Ok(())
            }
        }
    }

    /// 分落点核销计数。
    pub fn done_of(&self, s: Site) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].site == s && self.items[k].done {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    /// 分落点总数。
    pub fn total_of(&self, s: Site) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].site == s {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    /// 悬空探测：全核销 = done 总数等于件总数。
    pub fn no_dangling(&self) -> bool {
        let mut done = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].done {
                done = done.wrapping_add(1);
            }
            k += 1;
        }
        done == self.items.len() as u32
    }

    /// 核销对平：两落点 done 之和与逐件扫描一致且各落点 done ≤ total。
    pub fn balanced(&self) -> bool {
        let mut done = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].done {
                done = done.wrapping_add(1);
            }
            k += 1;
        }
        done == self.done_of(Site::IpcStress).wrapping_add(self.done_of(Site::VimFull))
            && self.done_of(Site::IpcStress) <= self.total_of(Site::IpcStress)
            && self.done_of(Site::VimFull) <= self.total_of(Site::VimFull)
    }

    /// fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1ec2_02a7;
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = (h ^ it.no as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.done as u64).wrapping_mul(0x100000001b3);
            h = (h ^ match it.site { Site::IpcStress => 1u64, Site::VimFull => 2 })
                .wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 悬空判据必抓（无落点登记）
    let mut b = SiteBook::new();
    got[0] = b.enroll(1, None).err() == Some(LC_DANGLING) && b.items.is_empty();
    // 2) 重判据必抓
    let _ = b.enroll(2, Some(Site::IpcStress));
    got[1] = b.enroll(2, Some(Site::VimFull)).err() == Some(LC_DUP);
    // 3) 未知判据与重复核销必抓
    let mut c = SiteBook::new();
    got[2] = c.verify(9).err() == Some(LC_NOENT);
    let _ = c.enroll(3, Some(Site::VimFull));
    let _ = c.verify(3);
    got[2] = got[2] && c.verify(3).err() == Some(LC_TWICE);
    // 4) 封账写入必抓
    c.seal();
    got[3] = c.enroll(4, Some(Site::IpcStress)).err() == Some(LC_FROZEN)
        && c.verify(3).err() == Some(LC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11097_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11097");

    // J1 落点逐件核销对平
    let mut b = SiteBook::new();
    // IPC 压测落点 6 条 + vim 全屏落点 4 条。
    let mut n = 1u32;
    while n <= 6 {
        let _ = b.enroll(n, Some(Site::IpcStress));
        n = n.wrapping_add(1);
    }
    let mut n = 7u32;
    while n <= 10 {
        let _ = b.enroll(n, Some(Site::VimFull));
        n = n.wrapping_add(1);
    }
    let mut n = 1u32;
    while n <= 10 {
        let _ = b.verify(n);
        n = n.wrapping_add(1);
    }
    let inj = inject_four();
    let j1 = b.done_of(Site::IpcStress) == 6 && b.done_of(Site::VimFull) == 4
        && b.balanced() && inj[1];
    s.add("J1-落点逐件核销", j1, "两落点十件判据逐件核销计数对平");

    // J2 验收判据零悬空
    let j2 = b.no_dangling() && b.items.len() == 10 && inj[0];
    s.add("J2-零悬空", j2, "全件核销悬空探测为真且空落点登记必拒");

    // J3 悬空判据必抓
    s.add("J3-悬空抓", inj[0], "零落点登记必拒不入册");

    // J4 重判据必抓
    s.add("J4-重号抓", inj[1], "判据重号必拒");

    // J5 未知判据与重复核销必抓
    s.add("J5-未知重销抓", inj[2], "未知判据核销与重复核销必拒");

    // J6 封账写入必抓
    s.add("J6-封账拒写", inj[3], "封账后登记与核销两路均必拒");

    // J7 漏核销探测：部分核销悬空为假
    let mut c = SiteBook::new();
    let mut n = 1u32;
    while n <= 5 {
        let _ = c.enroll(n, Some(Site::IpcStress));
        n = n.wrapping_add(1);
    }
    let _ = c.verify(1);
    let _ = c.verify(2);
    let j7 = !c.no_dangling() && c.done_of(Site::IpcStress) == 2
        && c.total_of(Site::IpcStress) == 5;
    s.add("J7-漏销探测", j7, "部分核销悬空裁断为假可定位漏件");

    // J8 落点隔离：vim 落点核销不影响 IPC 计数
    let mut d = SiteBook::new();
    let _ = d.enroll(1, Some(Site::IpcStress));
    let _ = d.enroll(2, Some(Site::VimFull));
    let _ = d.verify(2);
    let j8 = d.done_of(Site::IpcStress) == 0 && d.done_of(Site::VimFull) == 1
        && d.balanced();
    s.add("J8-落点隔离", j8, "分落点计数互不串账");

    // J9 fold 指纹与封账根
    let fp0 = b.fold();
    let root = b.seal();
    let j9 = fp0 == root && b.fold() == root;
    s.add("J9-指纹冻结", j9, "封账根与 fold 一致且二次 fold 稳定");

    // J10 重建幂等
    let mut e = SiteBook::new();
    let mut n = 1u32;
    while n <= 6 {
        let _ = e.enroll(n, Some(Site::IpcStress));
        n = n.wrapping_add(1);
    }
    let mut n = 7u32;
    while n <= 10 {
        let _ = e.enroll(n, Some(Site::VimFull));
        n = n.wrapping_add(1);
    }
    let mut n = 1u32;
    while n <= 10 {
        let _ = e.verify(n);
        n = n.wrapping_add(1);
    }
    let j10 = e.fold() == fp0;
    s.add("J10-重建幂等", j10, "同构重建落点账指纹一致");

    // J11 核销计数明细落账
    s.add_dyn("J11-核销明细", b.done_of(Site::IpcStress) == 6,
        &format!("ipc={} vim={}", b.done_of(Site::IpcStress), b.done_of(Site::VimFull)));

    // J12 静态判据：IPC 落点核销计数为正
    s.add("J12-核销非零", b.done_of(Site::IpcStress) > 0, "IPC 落点核销项数恒为正");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11097_site_flow() {
        let mut b = SiteBook::new();
        assert!(b.enroll(1, None).err() == Some(LC_DANGLING));
        assert!(b.enroll(1, Some(Site::IpcStress)).is_ok());
        assert!(b.enroll(1, Some(Site::VimFull)).err() == Some(LC_DUP));
        assert!(b.verify(9).err() == Some(LC_NOENT));
        assert!(b.verify(1).is_ok());
        assert!(b.verify(1).err() == Some(LC_TWICE));
        // no_dangling 的语义是「全核销」（done 总数 == 件总数，见 J2 与
        // J7 两处判据）。此刻唯一一件判据已核销，故必须为 true；初版写成
        // `!no_dangling()`，与该方法自己的语义及 J2 判据直接矛盾。
        assert!(b.no_dangling());
        b.seal();
        assert!(b.verify(1).err() == Some(LC_FROZEN));
        assert!(b.enroll(2, Some(Site::VimFull)).err() == Some(LC_FROZEN));
    }

    #[test]
    fn unx11097_checks_green() {
        let s = run_unx11097_checks();
        assert!(s.all_passed(), "UNX-F11097 判据未全绿");
    }
}
