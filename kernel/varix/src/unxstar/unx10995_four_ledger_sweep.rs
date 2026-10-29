//! UNX-F10995 四账型资源总扫（fd/内存/进程表/IPC）（0x1dbe0 段）
//!
//! 锚点：UNX-F10995-J1 场毕四账归零——fd 账/内存账/进程账/IPC 账四族在
//! 场景收官时逐族余额归零；J2 分段留痕可定位——每族账目分段留痕，任一
//! 段不平可精确定位到段号与族名。
//! 域：CoRun·UNX-C4（多进程压力域）。
//!
//! 结构：四族账本（统一 SegLedger：族枚举 + 分段余额表 + 段留痕）+
//! 总扫引擎（逐段逐族核扫，收集不平衡段清单）+ 归零裁断 + 注入核
//! （族错扫/段越界/重开段/封账写入四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1dbe0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1dbe0;
pub const RS_NOENT: u32 = SEG_BASE + 0x01; // 段不在账
pub const RS_DUP: u32 = SEG_BASE + 0x02; // 段重开
pub const RS_RANGE: u32 = SEG_BASE + 0x03; // 段号越界
pub const RS_FROZEN: u32 = SEG_BASE + 0x04; // 封账后写账
pub const RS_KIND: u32 = SEG_BASE + 0x05; // 族错扫
pub const RS_NOTZERO: u32 = SEG_BASE + 0x06; // 归零裁断时余额非零
pub const RS_TRACE: u32 = SEG_BASE + 0x07; // 留痕缺失

/// 四族账型。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fam {
    Fd,
    Mem,
    Proc,
    Ipc,
}

/// 分段留痕：段号 + 族 + 加减笔数 + 末余额。
#[derive(Clone, Debug)]
pub struct SegMark {
    pub seg: u32,
    pub fam: Fam,
    pub ups: u32,
    pub downs: u32,
    pub last: u32,
}

/// 分段账项：族 + 段内余额。
#[derive(Clone, Debug)]
pub struct FamSeg {
    pub fam: Fam,
    pub seg: u32,
    pub bal: u32,
}

/// 四族分段账本。
pub struct SegLedger {
    pub segs: Vec<FamSeg>,
    pub marks: Vec<SegMark>,
    pub sealed: bool,
}

impl SegLedger {
    pub fn new() -> SegLedger {
        SegLedger { segs: Vec::new(), marks: Vec::new(), sealed: false }
    }

    fn find(&self, fam: Fam, seg: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].fam == fam && self.segs[k].seg == seg {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 开段：封账/重开/越界（段号 0 或 >4096）必拒。
    pub fn open_seg(&mut self, fam: Fam, seg: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RS_FROZEN);
        }
        if seg == 0 || seg > 4096 {
            return Err(RS_RANGE);
        }
        if self.find(fam, seg).is_some() {
            return Err(RS_DUP);
        }
        self.segs.push(FamSeg { fam, seg, bal: 0 });
        Ok(())
    }

    /// 族加账（留痕）。
    pub fn add(&mut self, fam: Fam, seg: u32, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(RS_FROZEN);
        }
        let i = match self.find(fam, seg) {
            Some(i) => i,
            None => return Err(RS_NOENT),
        };
        self.segs[i].bal = self.segs[i].bal.wrapping_add(n);
        self.trace(fam, seg, 1, 0, self.segs[i].bal);
        Ok(self.segs[i].bal)
    }

    /// 族减账（留痕；余额不足必拒）。
    pub fn sub(&mut self, fam: Fam, seg: u32, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(RS_FROZEN);
        }
        let i = match self.find(fam, seg) {
            Some(i) => i,
            None => return Err(RS_NOENT),
        };
        if self.segs[i].bal < n {
            return Err(RS_NOTZERO);
        }
        self.segs[i].bal = self.segs[i].bal.wrapping_sub(n);
        self.trace(fam, seg, 0, 1, self.segs[i].bal);
        Ok(self.segs[i].bal)
    }

    fn trace(&mut self, fam: Fam, seg: u32, up: u32, down: u32, last: u32) {
        self.marks.push(SegMark { seg, fam, ups: up, downs: down, last });
    }

    /// 段余额查询（族错扫守卫：查时必须带同族）。
    pub fn bal(&self, fam: Fam, seg: u32) -> Result<u32, u32> {
        match self.find(fam, seg) {
            Some(i) => Ok(self.segs[i].bal),
            None => Err(RS_NOENT),
        }
    }

    /// 总扫：收集非零段清单（段号+族）。
    pub fn sweep(&self) -> Vec<(Fam, u32)> {
        let mut bad: Vec<(Fam, u32)> = Vec::new();
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].bal != 0 {
                bad.push((self.segs[k].fam, self.segs[k].seg));
            }
            k += 1;
        }
        bad
    }

    /// 归零裁断：全段零余额。
    pub fn all_zero(&self) -> bool {
        self.sweep().is_empty()
    }

    /// 留痕完备核验：每段至少一笔留痕且末余额与现余额一致。
    pub fn traces_ok(&self) -> bool {
        let mut k = 0usize;
        while k < self.segs.len() {
            let (f, s) = (self.segs[k].fam, self.segs[k].seg);
            let mut seen = false;
            let mut last = 0u32;
            let mut m = 0usize;
            while m < self.marks.len() {
                if self.marks[m].fam == f && self.marks[m].seg == s {
                    seen = true;
                    last = self.marks[m].last;
                }
                m += 1;
            }
            if !seen || last != self.segs[k].bal {
                return false;
            }
            k += 1;
        }
        true
    }

    /// fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1dbe_0295;
        let mut k = 0usize;
        while k < self.segs.len() {
            h = (h ^ (self.segs[k].seg as u64) << 8).wrapping_mul(0x100000001b3);
            h = (h ^ self.segs[k].bal as u64).wrapping_mul(0x100000001b3);
            h = (h ^ match self.segs[k].fam { Fam::Fd => 1u64, Fam::Mem => 2, Fam::Proc => 3, Fam::Ipc => 4 })
                .wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.marks.len() as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 场景模拟：四族各开 3 段，各段加账 N 笔再等额减账，场毕全零。
pub fn scenario_rounds(n: u32) -> (SegLedger, bool) {
    let mut l = SegLedger::new();
    let fams = [Fam::Fd, Fam::Mem, Fam::Proc, Fam::Ipc];
    let mut f = 0usize;
    while f < fams.len() {
        let mut seg = 1u32;
        while seg <= 3 {
            let _ = l.open_seg(fams[f], seg);
            seg = seg.wrapping_add(1);
        }
        f += 1;
    }
    // 加账阶段。
    let mut f = 0usize;
    while f < fams.len() {
        let mut seg = 1u32;
        while seg <= 3 {
            let mut r = 0u32;
            while r < n {
                let _ = l.add(fams[f], seg, 1);
                r = r.wrapping_add(1);
            }
            seg = seg.wrapping_add(1);
        }
        f += 1;
    }
    // 减账阶段（等额）。
    let mut f = 0usize;
    while f < fams.len() {
        let mut seg = 1u32;
        while seg <= 3 {
            let mut r = 0u32;
            while r < n {
                let _ = l.sub(fams[f], seg, 1);
                r = r.wrapping_add(1);
            }
            seg = seg.wrapping_add(1);
        }
        f += 1;
    }
    let l_ok = l.all_zero() && l.traces_ok();
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 段越界必抓
    let mut b = SegLedger::new();
    got[0] = b.open_seg(Fam::Fd, 0).err() == Some(RS_RANGE)
        && b.open_seg(Fam::Fd, 4097).err() == Some(RS_RANGE);
    // 2) 段重开必抓
    let _ = b.open_seg(Fam::Mem, 2);
    got[1] = b.open_seg(Fam::Mem, 2).err() == Some(RS_DUP);
    // 3) 族错扫必抓：查不存在的族段
    let _ = b.open_seg(Fam::Ipc, 3);
    let _ = b.add(Fam::Ipc, 3, 5);
    got[2] = b.bal(Fam::Fd, 3).err() == Some(RS_NOENT)
        && b.bal(Fam::Proc, 3).err() == Some(RS_NOENT);
    // 4) 封账写入必抓
    b.seal();
    got[3] = b.open_seg(Fam::Fd, 9).err() == Some(RS_FROZEN)
        && b.add(Fam::Ipc, 3, 1).err() == Some(RS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10995_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10995");

    // J1 场毕四账归零
    let (l, ok) = scenario_rounds(100);
    let inj = inject_four();
    let j1 = ok && l.all_zero() && inj[0];
    s.add("J1-场毕四账归零", j1, "fd/内存/进程/IPC 四族段场毕全零");

    // J2 分段留痕可定位
    let mut b = SegLedger::new();
    let _ = b.open_seg(Fam::Fd, 1);
    let _ = b.open_seg(Fam::Mem, 2);
    let _ = b.add(Fam::Fd, 1, 3);
    let _ = b.add(Fam::Mem, 2, 1);
    let _ = b.sub(Fam::Mem, 2, 1);
    let bad = b.sweep();
    let j2 = bad.len() == 1 && bad[0] == (Fam::Fd, 1) && b.traces_ok();
    s.add("J2-分段留痕定位", j2, "非零段清单精确到段号与族名");

    // J3 段越界与重开必抓
    s.add("J3-越界重开抓", inj[0] && inj[1], "段号越界与重开两路必抓");

    // J4 族错扫必抓
    s.add("J4-族错扫抓", inj[2], "跨族查询必报无段不串账");

    // J5 封账写入必抓
    s.add("J5-封账拒写", inj[3], "封账后开段与加减账均必拒");

    // J6 减账透支必拒且账不动
    let mut c = SegLedger::new();
    let _ = c.open_seg(Fam::Proc, 1);
    let over = c.sub(Fam::Proc, 1, 2);
    let _ = c.add(Fam::Proc, 1, 1);
    let over2 = c.sub(Fam::Proc, 1, 2);
    let j6 = over.err() == Some(RS_NOTZERO) && over2.err() == Some(RS_NOTZERO)
        && c.bal(Fam::Proc, 1) == Ok(1);
    s.add("J6-透支拒", j6, "余额不足减账必拒且账面不透支");

    // J7 部分归零：减超不报错按需减
    let mut d = SegLedger::new();
    let _ = d.open_seg(Fam::Ipc, 4);
    let _ = d.add(Fam::Ipc, 4, 5);
    let got = d.sub(Fam::Ipc, 4, 5);
    let j7 = got == Ok(0) && d.bal(Fam::Ipc, 4) == Ok(0) && d.all_zero();
    s.add("J7-等额归零", j7, "等额减账后段归零且裁断为真");

    // J8 留痕完备：漏留痕段被核验抓出
    let mut e = SegLedger::new();
    let _ = e.open_seg(Fam::Fd, 1);
    let _ = e.open_seg(Fam::Fd, 2);
    let _ = e.add(Fam::Fd, 1, 2);
    // 段 2 无留痕。
    let j8 = !e.traces_ok() && e.segs.len() == 2;
    s.add("J8-漏痕抓", j8, "无留痕段必被留痕核验抓出");

    // J9 未知段加减必拒
    let j9 = SegLedger::new().add(Fam::Mem, 7, 1).err() == Some(RS_NOENT);
    s.add("J9-未知段拒", j9, "未开段加减必拒");

    // J10 大场景对平：千段四族全零
    let mut g = SegLedger::new();
    let fams = [Fam::Fd, Fam::Mem, Fam::Proc, Fam::Ipc];
    let mut f = 0usize;
    let mut opened = true;
    while f < fams.len() {
        let mut seg = 1u32;
        while seg <= 250 {
            if g.open_seg(fams[f], seg).is_err() {
                opened = false;
            }
            let _ = g.add(fams[f], seg, 2);
            let _ = g.sub(fams[f], seg, 1);
            let _ = g.sub(fams[f], seg, 1);
            seg = seg.wrapping_add(1);
        }
        f += 1;
    }
    let j10 = opened && g.all_zero() && g.traces_ok() && g.segs.len() == 1000;
    s.add("J10-千段对平", j10, "千段四族加等额减全零且留痕完备");

    // J11 fold 指纹与封账根
    let fp0 = b.fold();
    let root = b.seal();
    let j11 = fp0 == root && b.fold() == root;
    s.add("J11-指纹冻结", j11, "封账根与 fold 一致且二次 fold 稳定");

    // J12 动态明细：千段场景段数与留痕数落账
    s.add_dyn("J12-规模明细", g.segs.len() == 1000,
        &format!("segs={} marks={}", g.segs.len(), g.marks.len()));

    // J13 动态明细：场景账指纹落账
    let fp = l.fold();
    s.add_dyn("J13-账指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10995_sweep_flow() {
        let mut l = SegLedger::new();
        assert!(l.open_seg(Fam::Fd, 1).is_ok());
        assert!(l.open_seg(Fam::Fd, 1).err() == Some(RS_DUP));
        assert!(l.open_seg(Fam::Fd, 0).err() == Some(RS_RANGE));
        assert!(l.add(Fam::Fd, 1, 4).is_ok());
        assert!(l.sub(Fam::Fd, 1, 5).err() == Some(RS_NOTZERO));
        assert!(l.sub(Fam::Fd, 1, 4) == Ok(0));
        assert!(l.all_zero());
        assert!(l.traces_ok());
        l.seal();
        assert!(l.add(Fam::Fd, 1, 1).err() == Some(RS_FROZEN));
    }

    #[test]
    fn unx10995_checks_green() {
        let s = run_unx10995_checks();
        assert!(s.all_passed(), "UNX-F10995 判据未全绿");
    }
}
