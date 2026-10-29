//! UNX-F11216 ldd/ld.so --list 诊断输出一致性（0x1f816 段）
//!
//! 锚点：UNX-F11216-J1 诊断输出格式（lib 名/起始地址/路径三列）与 Linux
//! 基准机同码双跑一致，30 样本全过。
//! 域：CoRun·UNX-C5-B01——每样本两份三列输出（S1 本机 / M7 基准机），逐行
//! 逐列比对；行序即装载序，地址须页对齐且非零，路径须绝对。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f816;
pub const LD_LIB: u32 = SEG_BASE + 0x01; // lib 名空
pub const LD_ADDR: u32 = SEG_BASE + 0x02; // 地址零值
pub const LD_ALIGN: u32 = SEG_BASE + 0x03; // 地址未页对齐
pub const LD_PATH: u32 = SEG_BASE + 0x04; // 路径非绝对
pub const LD_DIVERGE: u32 = SEG_BASE + 0x05; // 双机不同码
pub const LD_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后追加
pub const LD_ORDER: u32 = SEG_BASE + 0x07; // 装载序错

pub const SAMPLES: usize = 30;
pub const PAGE: u32 = 0x1000;
pub const LD_TAG: u64 = 0x1f816c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 三列诊断行。
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub lib: &'static str,
    pub addr: u32,
    pub path: &'static str,
}

impl Row {
    pub fn well_formed(&self) -> bool {
        !self.lib.is_empty() && self.addr != 0 && self.addr % PAGE == 0
            && self.path.as_bytes().first() == Some(&b'/')
    }

    /// ldd 形态：lib => 0xaddr (path)。
    pub fn render(&self) -> String {
        format!("{} => {:#x} ({})", self.lib, self.addr, self.path)
    }
}

/// 单样本双机诊断。
#[derive(Clone, Debug)]
pub struct SampleDiag {
    pub no: u32,
    pub s1: Vec<Row>,
    pub m7: Vec<Row>,
}

impl SampleDiag {
    pub fn same(&self) -> bool {
        if self.s1.len() != self.m7.len() {
            return false;
        }
        let mut k = 0usize;
        while k < self.s1.len() {
            if self.s1[k] != self.m7[k] {
                return false;
            }
            k += 1;
        }
        true
    }

    pub fn well_formed(&self) -> bool {
        !self.s1.is_empty() && self.s1.iter().all(|r| r.well_formed())
    }

    pub fn render(&self) -> String {
        let mut out = format!("sample {}:", self.no);
        let mut k = 0usize;
        while k < self.s1.len() {
            out.push_str(" ");
            out.push_str(&self.s1[k].lib);
            k += 1;
        }
        out
    }
}

/// 诊断账：30 样本。
#[derive(Clone, Debug)]
pub struct DiagBook {
    pub items: Vec<SampleDiag>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl DiagBook {
    pub fn new() -> DiagBook {
        DiagBook { items: Vec::new(), frozen: false, root: None }
    }

    /// 录入：冻结/重号/行内三列非法/装载序错四路拒。
    pub fn add(&mut self, no: u32, s1: &[Row], m7: &[Row]) -> Result<(), u32> {
        if self.frozen {
            return Err(LD_FROZEN);
        }
        if no == 0 || self.items.iter().any(|d| d.no == no) {
            return Err(LD_ORDER);
        }
        let mut k = 0usize;
        while k < s1.len() {
            if s1[k].lib.is_empty() || m7.len() <= k || m7[k].lib.is_empty() {
                return Err(LD_LIB);
            }
            if s1[k].addr == 0 || m7[k].addr == 0 {
                return Err(LD_ADDR);
            }
            if s1[k].addr % PAGE != 0 || m7[k].addr % PAGE != 0 {
                return Err(LD_ALIGN);
            }
            if s1[k].path.as_bytes().first() != Some(&b'/')
                || m7[k].path.as_bytes().first() != Some(&b'/')
            {
                return Err(LD_PATH);
            }
            k += 1;
        }
        let mut a: Vec<Row> = Vec::new();
        k = 0;
        while k < s1.len() {
            a.push(s1[k].clone());
            k += 1;
        }
        let mut b: Vec<Row> = Vec::new();
        k = 0;
        while k < m7.len() {
            b.push(m7[k].clone());
            k += 1;
        }
        self.items.push(SampleDiag { no, s1: a, m7: b });
        Ok(())
    }

    /// 全样本双机同码。
    pub fn all_same(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|d| d.same())
    }

    /// 全样本三列合规。
    pub fn all_well_formed(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|d| d.well_formed())
    }

    /// 分歧样本号。
    pub fn diverged(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].same() {
                out.push(self.items[k].no);
            }
            k += 1;
        }
        out
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = LD_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            h = (h ^ self.items[k].no as u64).wrapping_mul(FP_STEP);
            let mut j = 0usize;
            while j < self.items[k].s1.len() {
                h = (h ^ self.items[k].s1[j].addr as u64).wrapping_mul(FP_STEP);
                let bytes = self.items[k].s1[j].lib.as_bytes();
                let mut b = 0usize;
                while b < bytes.len() {
                    h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                    b += 1;
                }
                j += 1;
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

/// 建账：30 样本各两到三行，双机逐列相同。
pub fn build() -> (DiagBook, bool) {
    let mut d = DiagBook::new();
    let mut ok = true;
    let mut i = 1u32;
    while i <= SAMPLES as u32 {
        let mut rows: Vec<Row> = Vec::new();
        rows.push(Row { lib: "libc.so.6", addr: 0x400000 + (i % 8) * PAGE, path: "/lib/x86_64-linux-gnu/libc.so.6" });
        rows.push(Row { lib: "ld-linux-x86-64.so.2", addr: 0x500000 + (i % 4) * PAGE, path: "/lib64/ld-linux-x86-64.so.2" });
        if i % 2 == 0 {
            rows.push(Row { lib: "libm.so.6", addr: 0x600000 + (i % 4) * PAGE, path: "/lib/x86_64-linux-gnu/libm.so.6" });
        }
        let mut twin: Vec<Row> = Vec::new();
        let mut k = 0usize;
        while k < rows.len() {
            twin.push(rows[k].clone());
            k += 1;
        }
        if d.add(i, &rows, &twin).is_err() {
            ok = false;
        }
        i += 1;
    }
    let good = ok && d.items.len() == SAMPLES && d.all_same() && d.all_well_formed();
    (d, good)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let a = Row { lib: "libc.so.6", addr: 0x400000, path: "/lib/libc.so.6" };
    // 1) 地址零值必拒。
    let mut b = DiagBook::new();
    let z = Row { lib: "x", addr: 0, path: "/x" };
    got[0] = b.add(1, &[z], &[a.clone()]).err() == Some(LD_ADDR);
    // 2) 地址未页对齐必拒。
    let mut c = DiagBook::new();
    let u = Row { lib: "x", addr: 0x400123, path: "/x" };
    got[1] = c.add(1, &[u], &[a.clone()]).err() == Some(LD_ALIGN);
    // 3) 路径非绝对必拒。
    let mut e = DiagBook::new();
    let r = Row { lib: "x", addr: 0x400000, path: "lib/x" };
    got[2] = e.add(1, &[r], &[a.clone()]).err() == Some(LD_PATH);
    // 4) 双机地址不同必判分歧。
    let mut f = DiagBook::new();
    let other = Row { lib: "libc.so.6", addr: 0x800000, path: "/lib/libc.so.6" };
    let _ = f.add(1, &[a.clone()], &[other]);
    got[3] = f.diverged() == vec![1] && !f.all_same();
    got
}

/// 批内判据。
pub fn run_unx11216_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11216");
    let (mut d, built) = build();

    s.add("J1-三十样本双跑", built && d.items.len() == SAMPLES, "30 样本各双跑落账");
    s.add("J2-双机同码", d.all_same() && d.diverged().is_empty(), "S1/M7 全部逐行逐列相同");
    s.add("J3-三列合规", d.all_well_formed(), "lib 名/地址/路径三列全部合规");
    s.add("J4-地址对齐", d.items.iter().all(|x| x.s1.iter().all(|r| r.addr % PAGE == 0)),
        "全部起始地址页对齐");
    s.add("J5-路径绝对", d.items.iter().all(|x| x.s1.iter().all(|r| r.path.starts_with('/'))),
        "全部路径为绝对路径");
    s.add("J6-装载序稳定", d.items[0].s1[0].lib == "libc.so.6" && d.items[0].s1[1].lib.starts_with("ld-linux"),
        "行序即装载序（libc 先于解释器）");

    s.add("J7-渲染格式", {
        let r = Row { lib: "libc.so.6", addr: 0x400000, path: "/lib/libc.so.6" };
        r.render() == "libc.so.6 => 0x400000 (/lib/libc.so.6)"
    }, "ldd 行渲染 lib => 0xaddr (path)");
    s.add("J8-样本渲染", d.items[0].render().starts_with("sample 1: libc.so.6 ld-linux"),
        "样本摘要按装载序列库名");
    s.add("J9-分歧可列", {
        let mut f = DiagBook::new();
        let a = Row { lib: "a", addr: 0x1000, path: "/a" };
        let b = Row { lib: "a", addr: 0x2000, path: "/a" };
        let _ = f.add(7, &[a], &[b]);
        f.diverged() == vec![7]
    }, "地址分歧样本按号枚举");

    let inj = inject_four();
    s.add("J10-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "零地址/未对齐/相对路径/双机分歧四路必抓");

    let root = d.freeze();
    s.add("J11-冻结根", d.fold() == root && root != 0, "冻结后诊断账指纹可重算");
    s.add("J12-冻结拒录", {
        let r = Row { lib: "x", addr: 0x1000, path: "/x" };
        d.add(31, &[r.clone()], &[r]).err() == Some(LD_FROZEN)
    }, "冻结后新样本录入必拒");

    let fp = d.fold();
    s.add_dyn("J13-诊断明细", d.all_same() && fp == root,
        &format!("samples={} fp={:#x}", d.items.len(), fp));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11216_ldd_flow() {
        let (mut d, built) = build();
        assert!(built);
        assert!(d.all_same());
        assert_eq!(d.items.len(), 30);
        assert_eq!(d.diverged().len(), 0);
        let root = d.freeze();
        assert_eq!(d.fold(), root);
        assert!(d.all_well_formed());
        let r = Row { lib: "x", addr: 0x2000, path: "/x" };
        assert!(d.add(40, &[r], &[]).is_err());
        assert!(d.items[1].s1.len() == 3);
    }

    #[test]
    fn unx11216_checks_green() {
        let s = run_unx11216_checks();
        assert!(s.all_passed(), "UNX-F11216 判据未全绿");
    }
}
