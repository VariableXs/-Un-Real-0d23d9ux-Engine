//! UNX-F11093 长稳数据完整性对账（全程守恒总账）（0x1eba0 段）
//!
//! 锚点：UNX-F11093-J1 全程守恒账对平——写入总量 = 读出总量 + 在途余量
//! 的全程守恒式万轮闭合；J2 分段-全程账闭合——每分段账之和等于全程
//! 总账，分段可并归。
//! 域：CoRun·UNX-C4（长稳监控域）。
//!
//! 结构：守恒账（写计数/读计数/在途余额三分量）+ 分段账（段内三分量
//! 独立记账，段间可并归）+ 对账器（守恒式闭合裁断）+ 注入核（读超写/
//! 分段越界/段并错配/封账写入四路必抓）。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x1eba0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1eba0;
pub const CI_OVER: u32 = SEG_BASE + 0x01; // 读超写在途
pub const CI_RANGE: u32 = SEG_BASE + 0x02; // 分段越界
pub const CI_MERGE: u32 = SEG_BASE + 0x03; // 段并错配
pub const CI_FROZEN: u32 = SEG_BASE + 0x04; // 封账后写账
pub const CI_GAP: u32 = SEG_BASE + 0x05; // 分段缺口

/// 守恒三分量。
#[derive(Clone, Copy, Debug)]
pub struct Flow {
    pub written: u64,
    pub read: u64,
    pub inflight: u64,
}

impl Flow {
    pub fn new() -> Flow {
        Flow { written: 0, read: 0, inflight: 0 }
    }

    /// 写入 n。
    pub fn write(&mut self, n: u64) {
        self.written = self.written.wrapping_add(n);
        self.inflight = self.inflight.wrapping_add(n);
    }

    /// 读出 n（不得超在途）。
    pub fn take(&mut self, n: u64) -> Result<u64, u32> {
        if self.inflight < n {
            return Err(CI_OVER);
        }
        self.read = self.read.wrapping_add(n);
        self.inflight = self.inflight.wrapping_sub(n);
        Ok(n)
    }

    /// 守恒式：written = read + inflight。
    pub fn conserved(&self) -> bool {
        self.written == self.read.wrapping_add(self.inflight)
    }
}

/// 分段账项：段号 + 三分量。
#[derive(Clone, Copy, Debug)]
pub struct SegFlow {
    pub seg: u32,
    pub flow: Flow,
}

/// 全程账。
pub struct ConsistBook {
    pub total: Flow,
    pub segs: Vec<SegFlow>,
    pub sealed: bool,
}

impl ConsistBook {
    pub fn new() -> ConsistBook {
        ConsistBook { total: Flow::new(), segs: Vec::new(), sealed: false }
    }

    /// 开分段（段号升序，重号必拒）。
    pub fn open(&mut self, seg: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CI_FROZEN);
        }
        if seg == 0 {
            return Err(CI_RANGE);
        }
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].seg == seg {
                return Err(CI_MERGE);
            }
            k = k.wrapping_add(1);
        }
        self.segs.push(SegFlow { seg, flow: Flow::new() });
        Ok(())
    }

    fn seg_at(&self, seg: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.segs.len() {
            if self.segs[k].seg == seg {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 分段写（全程同步记账）。
    pub fn write(&mut self, seg: u32, n: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(CI_FROZEN);
        }
        match self.seg_at(seg) {
            None => Err(CI_RANGE),
            Some(i) => {
                self.segs[i].flow.write(n);
                self.total.write(n);
                Ok(())
            }
        }
    }

    /// 分段读（全程同步记账）。
    pub fn take(&mut self, seg: u32, n: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(CI_FROZEN);
        }
        match self.seg_at(seg) {
            None => Err(CI_RANGE),
            Some(i) => {
                let got = self.segs[i].flow.take(n)?;
                let _ = self.total.take(n)?;
                Ok(got)
            }
        }
    }

    /// 分段-全程闭合：段三分量之和 = 全程三分量。
    pub fn seg_total_closed(&self) -> bool {
        let mut w = 0u64;
        let mut r = 0u64;
        let mut f = 0u64;
        let mut k = 0usize;
        while k < self.segs.len() {
            w = w.wrapping_add(self.segs[k].flow.written);
            r = r.wrapping_add(self.segs[k].flow.read);
            f = f.wrapping_add(self.segs[k].flow.inflight);
            k = k.wrapping_add(1);
        }
        w == self.total.written && r == self.total.read && f == self.total.inflight
    }

    /// 分段连续性：段号升序无缺口。
    pub fn segs_contig(&self) -> bool {
        let mut k = 1usize;
        while k < self.segs.len() {
            if self.segs[k].seg != self.segs[k - 1].seg + 1 {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 全裁断。
    pub fn all_ok(&self) -> bool {
        self.total.conserved() && self.seg_total_closed()
    }

    /// fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1eba_02a3;
        h = (h ^ self.total.written).wrapping_mul(0x100000001b3);
        h = (h ^ self.total.read).wrapping_mul(0x100000001b3);
        h = (h ^ self.total.inflight).wrapping_mul(0x100000001b3);
        h = (h ^ self.segs.len() as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 万轮守恒压力：分段写读交叉，全部闭合。
pub fn stress_flow(rounds: u32) -> (ConsistBook, bool) {
    let mut b = ConsistBook::new();
    let mut s = 1u32;
    while s <= 4 {
        let _ = b.open(s);
        s = s.wrapping_add(1);
    }
    let mut ok = true;
    let mut r = 0u32;
    while r < rounds {
        let seg = (r % 4).wrapping_add(1);
        let n = (r % 5).wrapping_add(1) as u64;
        if b.write(seg, n).is_err() {
            ok = false;
        }
        if b.take(seg, n).is_err() {
            ok = false;
        }
        if !b.all_ok() {
            ok = false;
        }
        r = r.wrapping_add(1);
    }
    let b_ok = ok && b.all_ok() && b.segs_contig();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 读超写在途必抓
    let mut f = Flow::new();
    f.write(2);
    got[0] = f.take(5).err() == Some(CI_OVER) && f.conserved();
    // 2) 分段越界必抓
    let mut b = ConsistBook::new();
    got[1] = b.write(9, 1).err() == Some(CI_RANGE)
        && b.take(9, 1).err() == Some(CI_RANGE)
        && b.open(0).err() == Some(CI_RANGE);
    // 3) 段并错配（重号段）必抓
    let _ = b.open(1);
    got[2] = b.open(1).err() == Some(CI_MERGE);
    // 4) 封账写入必抓
    b.seal();
    got[3] = b.write(1, 1).err() == Some(CI_FROZEN)
        && b.open(2).err() == Some(CI_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11093_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11093");

    // J1 全程守恒账对平
    let (mut b, ok) = stress_flow(10000);
    let inj = inject_four();
    let j1 = ok && b.total.conserved() && b.total.written == b.total.read
        && b.total.inflight == 0 && inj[0];
    s.add("J1-全程守恒对平", j1, "万轮写读交叉守恒式全程闭合");

    // J2 分段-全程账闭合
    let j2 = b.seg_total_closed() && b.segs.len() == 4 && b.segs_contig();
    s.add("J2-分段全程闭合", j2, "分段三分量之和与全程账逐项相等");

    // J3 读超写在途必抓
    s.add("J3-读超抓", inj[0], "在途不足读取必拒且守恒式不破");

    // J4 分段越界必抓
    s.add("J4-越界抓", inj[1], "未开段读写必拒");

    // J5 段重号必抓
    s.add("J5-重段抓", inj[2], "分段重开必拒");

    // J6 封账写入必抓
    s.add("J6-封账拒写", inj[3], "封账后写读开段三路均必拒");

    // J7 在途余额留痕：只写不读时在途非零
    let mut c = ConsistBook::new();
    let _ = c.open(1);
    let _ = c.write(1, 10);
    let j7 = c.total.inflight == 10 && c.total.read == 0 && c.total.conserved();
    s.add("J7-在途留痕", j7, "只写不读在途余额如实留痕");

    // J8 分段缺口抓：跳段登记破坏连续性
    let mut d = ConsistBook::new();
    let _ = d.open(1);
    let _ = d.open(3);
    let j8 = !d.segs_contig();
    s.add("J8-缺口抓", j8, "跳段使连续性裁断为假");

    // J9 乱序写读守恒：读后写再读全程闭合
    let mut e = ConsistBook::new();
    let _ = e.open(1);
    let _ = e.write(1, 4);
    let a = e.take(1, 2);
    let _ = e.write(1, 6);
    let bb = e.take(1, 8);
    let j9 = a == Ok(2) && bb == Ok(8) && e.total.conserved()
        && e.total.written == 10 && e.total.read == 10 && e.total.inflight == 0;
    s.add("J9-乱序闭合", j9, "写读交叉全程守恒式闭合");

    // J10 fold 指纹与封账根
    let fp0 = b.fold();
    let root = b.seal();
    let j10 = fp0 == root && b.fold() == root;
    s.add("J10-指纹冻结", j10, "封账根与 fold 一致且二次 fold 稳定");

    // J11 万轮规模明细落账
    s.add_dyn("J11-规模明细", b.total.written == b.total.read && b.total.written > 0,
        &format!("written={} read={} inflight={}", b.total.written, b.total.read, b.total.inflight));

    // J12 指纹落账
    let fp = b.fold();
    s.add_dyn("J12-账指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11093_flow_test() {
        let mut f = Flow::new();
        f.write(5);
        assert!(f.take(3).is_ok());
        assert!(f.take(3).err() == Some(CI_OVER));
        assert!(f.conserved());
        let mut b = ConsistBook::new();
        assert!(b.open(0).err() == Some(CI_RANGE));
        assert!(b.open(1).is_ok());
        assert!(b.open(1).err() == Some(CI_MERGE));
        assert!(b.write(2, 1).err() == Some(CI_RANGE));
        assert!(b.write(1, 4).is_ok());
        assert!(b.take(1, 9).err() == Some(CI_OVER));
        assert!(b.take(1, 4).is_ok());
        assert!(b.all_ok());
        b.seal();
        assert!(b.write(1, 1).err() == Some(CI_FROZEN));
    }

    #[test]
    fn unx11093_checks_green() {
        let s = run_unx11093_checks();
        assert!(s.all_passed(), "UNX-F11093 判据未全绿");
    }
}
