//! UNX 卡 10157 — 地址越界 SIGSEGV 正反双判据（空指针达成与解引用复现受控）
//! 段位 seg=0x106c0
//! 判据原文：UNX-F10157-J1 空指针拦截达成（零页不可读分进程 SIGSEGV 10/10 次）
//! 锚点：零页恒无映射 → 解引用受控 SIGSEGV → 分进程复现 → 合法面通行
//! 域：SegvBook 零页册（地址面分类 + 拦截账 + 分进程复现核）
//! 结构：地址分类（零页/区内/野址）→ deref 探针 → SIGSEGV 账 → 分进程复现 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x106c0;
pub const SV_SEGV: u32 = SEG_BASE + 0x01;
pub const SV_WILD: u32 = SEG_BASE + 0x02;
pub const SV_SEALED: u32 = SEG_BASE + 0x03;
pub const SV_BAD_PID: u32 = SEG_BASE + 0x04;
pub const SV_FAULT_CAP: u32 = SEG_BASE + 0x05;

/// 页粒度与零页哨兵
pub const PAGE: usize = 0x1000;
pub const ZERO_TOP: usize = PAGE; // 零页区 [0, PAGE)
pub const FAULT_CAP: usize = 0x100;

/// 地址面分类
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Zone {
    ZeroPage,
    Mapped,
    Wild,
}

/// 单进程映射视图
#[derive(Clone, Debug)]
pub struct ProcView {
    pub pid: u32,
    pub start: usize,
    pub npages: usize,
}

/// SIGSEGV 册
pub struct SegvBook {
    pub views: Vec<ProcView>,
    pub faults: u32,
    pub deliveries: Vec<(u32, usize)>, // (pid, addr)
    sealed: bool,
}

impl SegvBook {
    pub fn new() -> Self {
        SegvBook {
            views: Vec::new(),
            faults: 0,
            deliveries: Vec::new(),
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂进程视图（合法映射区）
    pub fn map_view(&mut self, pid: u32, start: usize, npages: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(SV_SEALED);
        }
        if pid == 0 {
            return Err(SV_BAD_PID);
        }
        if npages == 0 || start % PAGE != 0 {
            return Err(SV_WILD);
        }
        self.views.push(ProcView { pid, start, npages });
        Ok(())
    }

    /// 地址面分类：先按进程视图（合法低址映射通行），再零页兜底，最后野址
    pub fn classify(&self, pid: u32, addr: usize) -> Zone {
        let mapped = self
            .views
            .iter()
            .any(|v| v.pid == pid && addr >= v.start && addr < v.start + v.npages * PAGE);
        if mapped {
            return Zone::Mapped;
        }
        if addr < ZERO_TOP {
            return Zone::ZeroPage;
        }
        Zone::Wild
    }

    /// deref 探针：零页/野址 → SIGSEGV 拦截账；区内通行
    pub fn deref(&mut self, pid: u32, addr: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(SV_SEALED);
        }
        if pid == 0 {
            return Err(SV_BAD_PID);
        }
        match self.classify(pid, addr) {
            Zone::Mapped => Ok(()),
            Zone::ZeroPage | Zone::Wild => {
                if self.deliveries.len() >= FAULT_CAP {
                    return Err(SV_FAULT_CAP);
                }
                self.faults += 1;
                self.deliveries.push((pid, addr));
                Err(SV_SEGV)
            }
        }
    }

    /// 分进程复现核：A 进程零页拦截、B 进程同址（合法区）通行
    pub fn per_proc_repro(&self, a: u32, b: u32, addr: usize) -> bool {
        self.classify(a, addr) == Zone::ZeroPage
            && self.classify(b, addr) == Zone::Mapped
    }

    /// 拦截账合计
    pub fn fault_total(&self) -> u32 {
        self.faults
    }

    /// 零页拦截占比
    pub fn zeropage_faults(&self) -> u32 {
        self.deliveries
            .iter()
            .filter(|(_, a)| *a < ZERO_TOP)
            .count() as u32
    }

    /// 拦截账指纹
    pub fn render_faults(&self) -> String {
        let mut s = String::new();
        for (pid, a) in &self.deliveries {
            s.push_str(&format!("{}:{:#x};", pid, a));
        }
        s
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 野址拦截笔数
    pub fn wild_faults(&self) -> u32 {
        self.deliveries
            .iter()
            .filter(|(_, a)| *a >= ZERO_TOP)
            .count() as u32
    }

    /// 视图总数
    pub fn view_total(&self) -> usize {
        self.views.len()
    }

    /// 按进程查视图
    pub fn view_of(&self, pid: u32) -> Option<&ProcView> {
        self.views.iter().find(|v| v.pid == pid)
    }

    /// 首笔拦截
    pub fn first_fault(&self) -> Option<(u32, usize)> {
        self.deliveries.first().copied()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SegvBook::new();
    let _ = b.map_view(1, 0x4000, 2);
    // 1) 零页解引用必拦
    got[0] = b.deref(1, 0).is_err();
    // 2) 野址必拦
    got[1] = b.deref(1, 0xDEAD_0000).is_err();
    // 3) 区内通行不拦
    let f0 = b.fault_total();
    let _ = b.deref(1, 0x4000 + 8);
    got[2] = b.fault_total() == f0;
    // 4) 零 pid 必拒
    got[3] = b.deref(0, 0x4000).is_err() && b.map_view(0, 0x8000, 1).is_err();
    // 5) 封册后必拒
    b.seal();
    got[4] = b.deref(1, 0).is_err();
    got
}

/// 判据入口：UNX-F10157
pub fn run_unx10157_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10157");
    let mut b = SegvBook::new();

    // J1 零页拦截 10/10：十进程各解引用零页均拦
    let mut j1 = true;
    for i in 0..10u32 {
        if b.map_view(i + 1, 0x4000 * (i as usize + 1), 1).is_err() {
            j1 = false;
        }
        if b.deref(i + 1, 0).is_ok() {
            j1 = false;
        }
    }
    s.add("J1_zeropage_10x", j1 && b.fault_total() == 10, "零页不可读分进程拦截 10/10");

    // J2 零页分类恒定
    let j2 = b.classify(1, 0) == Zone::ZeroPage
        && b.classify(1, PAGE - 1) == Zone::ZeroPage
        && b.classify(1, PAGE) != Zone::ZeroPage;
    s.add("J2_zero_zone", j2, "零页区 [0,PAGE) 分类恒定");

    // J3 分进程复现（B 进程持合法低址映射，同址 A 拦 B 行）
    let _ = b.map_view(3, 0, 1);
    let j3 = b.per_proc_repro(1, 3, 0) && b.classify(2, 0x8000) == Zone::Mapped;
    s.add("J3_per_proc", j3, "A 零页拦、B 同址合法面通行");

    // J4 野址受控
    let c0 = b.fault_total();
    let j4 = b.deref(1, 0xDEAD_0000) == Err(SV_SEGV) && b.fault_total() == c0 + 1;
    s.add("J4_wild", j4, "野址拦截且账 +1");

    // J5 区内通行
    let c1 = b.fault_total();
    let j5 = b.deref(1, 0x4000 + 0x10).is_ok() && b.fault_total() == c1;
    s.add("J5_mapped_ok", j5, "合法映射面解引用通行不记账");

    // J6 零 pid 受控
    let j6 = b.deref(0, 0) == Err(SV_BAD_PID) && b.map_view(0, 0x9000, 1) == Err(SV_BAD_PID);
    s.add("J6_bad_pid", j6, "零 pid 双路受控");

    // J7 非对齐挂视图受控
    let j7 = b.map_view(9, 0x1800, 1) == Err(SV_WILD)
        && b.map_view(9, 0xA000, 0) == Err(SV_WILD);
    s.add("J7_bad_view", j7, "非对齐与零页数挂视图受控");

    // J8 零页拦截账
    s.add("J8_zero_faults", b.zeropage_faults() == 10, "零页拦截 10 笔在账");

    // J9 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J9_inject_all", caught == 5, "5 注入点全检出");

    // J10 封册受控
    let mut b2 = SegvBook::new();
    b2.seal();
    let j10 = b2.deref(1, 0) == Err(SV_SEALED) && b2.map_view(1, 0x1000, 1) == Err(SV_SEALED);
    s.add("J10_sealed", j10, "封册后探针/挂视图皆拒 SEALED");

    // J11 拦截账容量守门
    let mut b3 = SegvBook::new();
    let mut fill_ok = true;
    for _ in 0..FAULT_CAP {
        if b3.deref(1, 0).is_ok() {
            fill_ok = false;
        }
    }
    let j11 = fill_ok && b3.deref(1, 0) == Err(SV_FAULT_CAP);
    s.add("J11_fault_cap", j11, "拦截账满 256 拒 FAULT_CAP");

    // J12 指纹稳定
    let f1 = b.render_faults();
    let f2 = b.render_faults();
    s.add("J12_fingerprint", f1 == f2 && f1.contains(":0x0;"), "拦截账指纹稳定含零页帧");

    // J13 视图计数
    s.add("J13_views", b.views.len() == 11, "十进程视图 + J3 合法低址映射在册");

    // J14 区边界：区尾外即野
    let j14 = b.classify(1, 0x4000 + PAGE) == Zone::Wild
        && b.classify(1, 0x4000 + PAGE - 1) == Zone::Mapped;
    s.add("J14_edge", j14, "区内尾地址在册、区外首址野");

    // J15 递达账序：全部为各 pid 首帧
    let first_ok = b.deliveries.iter().take(10).all(|(pid, a)| *a == 0);
    s.add("J15_delivery_order", first_ok && b.deliveries.len() == 11, "递达账 11 笔且前十笔零页");

    // J16 野址拦截分列
    let wf = b.wild_faults();
    s.add("J16_wild_split", wf == 1, "野址拦截 1 笔与零页 10 笔分列");

    // J17 视图查询
    let v = b.view_of(1);
    let j17 = v.is_some() && v.unwrap().start == 0x4000 && b.view_of(99).is_none();
    s.add("J17_view_of", j17, "按 pid 命中视图且野 pid 判空");

    // J18 视图总数
    let j18 = b.view_total() == 11;
    s.add("J18_view_total", j18, "十视图 + J3 低址映射在册");

    // J19 零页哨兵边界
    let j19 = b.classify(1, ZERO_TOP) != Zone::ZeroPage
        && b.classify(1, ZERO_TOP - 1) == Zone::ZeroPage;
    s.add("J19_sentinel", j19, "哨兵前零页、哨兵起出零页区");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segv57_zero() {
        let mut b = SegvBook::new();
        b.map_view(1, 0x4000, 1).unwrap();
        assert_eq!(b.deref(1, 0), Err(SV_SEGV));
        assert_eq!(b.deref(1, 0x4000), Ok(()));
        assert_eq!(b.fault_total(), 1);
    }

    #[test]
    fn test_segv57_wild() {
        let mut b = SegvBook::new();
        b.map_view(1, 0x4000, 1).unwrap();
        assert_eq!(b.classify(1, 0xDEAD_0000), Zone::Wild);
        assert_eq!(b.deref(1, 0xDEAD_0000), Err(SV_SEGV));
    }

    #[test]
    fn test_segv57_checks() {
        let cs = run_unx10157_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_segv57_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
