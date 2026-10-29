//! UNX-F11263 非 PIE ET_EXEC 装载判据联签（0x1f3f0 段）
//! 纯功能模型：ET_EXEC 固定基址装载——段按 p_vaddr 原址落位、入口必须落在
//! 可执行段内；装载器判据与 C1 ET_EXEC 判据各自独立成立后联签，
//! 两判据结论相左即立案（联签冲突），冲突本身不掩盖单边结论。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f3f0 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f3f0;
/// 段虚拟地址越出固定基址窗口或未页对齐。
pub const EX_SEG: u32 = SEG_BASE + 0x01;
/// e_type 非 ET_EXEC(2)。
pub const EX_VER: u32 = SEG_BASE + 0x02;
/// 入口未落在可执行段内。
pub const EX_EXEC: u32 = SEG_BASE + 0x03;
/// 固定基址非法（非 0x400000 或段越窗）。
pub const EX_BASE: u32 = SEG_BASE + 0x04;
/// 装载器判据与 C1 判据联签冲突。
pub const EX_JOINT: u32 = SEG_BASE + 0x05;
/// 封册后写账。
pub const EX_FROZEN: u32 = SEG_BASE + 0x06;
/// 段表已满。
pub const EX_CAP: u32 = SEG_BASE + 0x07;

/// ET_EXEC。
pub const ET_EXEC: u16 = 2;
/// 固定基址（x86-64 传统非 PIE 落点）。
pub const FIX_BASE: u64 = 0x0040_0000;
/// 固定窗口上界（基址 + 16M）。
pub const WIN_HI: u64 = 0x0140_0000;
/// 段权限位。
pub const PF_X: u8 = 1;
pub const PF_W: u8 = 2;
pub const PF_R: u8 = 4;
/// 段表定容与下限。
pub const SEG_CAP: usize = 12;
pub const SEG_NEED: usize = 3;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_63;

/// 固定址段：虚拟地址即落位地址，无 bias。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FSeg {
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub flags: u8,
}

impl FSeg {
    pub fn end(&self) -> u64 {
        self.vaddr.wrapping_add(self.memsz)
    }

    pub fn covers(&self, a: u64) -> bool {
        a >= self.vaddr && a < self.end()
    }

    pub fn exec(&self) -> bool {
        self.flags & PF_X != 0
    }

    /// 渲染：`RX@400000+2000`。
    pub fn render(&self) -> String {
        let mut t = String::new();
        if self.flags & PF_R != 0 {
            t.push_str("R");
        }
        if self.flags & PF_W != 0 {
            t.push_str("W");
        }
        if self.flags & PF_X != 0 {
            t.push_str("X");
        }
        format!("{}@{:x}+{:x}", t, self.vaddr, self.memsz)
    }
}

/// 固定基址装载账：e_type/段表/入口 + 联签位。
#[derive(Clone, Debug)]
pub struct FixedLoad {
    pub e_type: u16,
    pub base: u64,
    pub entry: u64,
    pub segs: Vec<FSeg>,
    pub joint: Option<bool>,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl FixedLoad {
    pub fn new() -> FixedLoad {
        FixedLoad {
            e_type: 0,
            base: FIX_BASE,
            entry: 0,
            segs: Vec::new(),
            joint: None,
            accepts: 0,
            rejects: 0,
            frozen: false,
            root: None,
        }
    }

    /// 登记固定址段。判定序：封册 → 页对齐 → 越窗 → 尺寸 → 重叠 → 定容。
    pub fn add_seg(&mut self, vaddr: u64, filesz: u64, memsz: u64, flags: u8)
        -> Result<usize, u32>
    {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EX_FROZEN);
        }
        if vaddr & 0xfff != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EX_SEG);
        }
        if vaddr < self.base || vaddr >= WIN_HI {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EX_BASE);
        }
        if filesz > memsz || memsz == 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EX_SEG);
        }
        let end = vaddr.wrapping_add(memsz);
        if end <= vaddr || end > WIN_HI {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EX_BASE);
        }
        let mut k = 0usize;
        while k < self.segs.len() {
            if let Some(o) = self.segs.get(k) {
                if vaddr < o.end() && o.vaddr < end {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(EX_SEG);
                }
            }
            k = k.wrapping_add(1);
        }
        if self.segs.len() >= SEG_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EX_CAP);
        }
        self.segs.push(FSeg { vaddr, filesz, memsz, flags });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.segs.len().wrapping_sub(1))
    }

    /// 定 e_type：只认 ET_EXEC。
    pub fn set_type(&mut self, t: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EX_FROZEN);
        }
        if t != ET_EXEC {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EX_VER);
        }
        self.e_type = t;
        self.accepts = self.accepts.wrapping_add(1);
        Ok(())
    }

    /// 定入口：必须落在某个可执行段内。
    pub fn set_entry(&mut self, e: u64) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EX_FROZEN);
        }
        let mut hit = false;
        let mut k = 0usize;
        while k < self.segs.len() {
            if let Some(s) = self.segs.get(k) {
                if s.exec() && s.covers(e) {
                    hit = true;
                }
            }
            k = k.wrapping_add(1);
        }
        if !hit {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EX_EXEC);
        }
        self.entry = e;
        self.accepts = self.accepts.wrapping_add(1);
        Ok(())
    }

    /// 装载器判据：类型/固定基址/入口在 X 段/段数下限/段序升。
    pub fn loader_ok(&self) -> bool {
        if self.e_type != ET_EXEC || self.base != FIX_BASE {
            return false;
        }
        if self.segs.len() < SEG_NEED {
            return false;
        }
        let mut prev = 0u64;
        let mut k = 0usize;
        while k < self.segs.len() {
            match self.segs.get(k) {
                Some(s) => {
                    if s.vaddr < prev {
                        return false;
                    }
                    prev = s.end();
                }
                None => return false,
            }
            k = k.wrapping_add(1);
        }
        let mut hit = false;
        let mut i = 0usize;
        while i < self.segs.len() {
            if let Some(s) = self.segs.get(i) {
                if s.exec() && s.covers(self.entry) {
                    hit = true;
                }
            }
            i = i.wrapping_add(1);
        }
        hit
    }

    /// C1 ET_EXEC 判据：基址页对齐且在窗口内、存在含入口的 X 段、尺寸非零。
    /// 与装载器判据**规则不同源**（不查 e_type/段数/段序），故可冲突。
    pub fn c1_ok(&self) -> bool {
        if self.base & 0xfff != 0 || self.base >= WIN_HI || self.base == 0 {
            return false;
        }
        let mut bytes = 0u64;
        let mut hit = false;
        let mut k = 0usize;
        while k < self.segs.len() {
            if let Some(s) = self.segs.get(k) {
                bytes = bytes.wrapping_add(s.memsz);
                if s.exec() && s.covers(self.entry) {
                    hit = true;
                }
            }
            k = k.wrapping_add(1);
        }
        bytes > 0 && hit
    }

    /// 联签：两判据结论一致才算签成，相左则以冲突入账。
    pub fn sign(&mut self) -> Result<bool, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EX_FROZEN);
        }
        let l = self.loader_ok();
        let c = self.c1_ok();
        if l != c {
            self.rejects = self.rejects.wrapping_add(1);
            self.joint = Some(false);
            return Err(EX_JOINT);
        }
        self.joint = Some(l);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(l)
    }

    /// FNV-1a 链指纹：类型/基址/入口/段序/联签位敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.e_type as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.base).wrapping_mul(FNV_PRIME);
        h = (h ^ self.entry).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.segs.len() {
            if let Some(s) = self.segs.get(k) {
                h = (h ^ s.vaddr).wrapping_mul(FNV_PRIME);
                h = (h ^ s.filesz).wrapping_mul(FNV_PRIME);
                h = (h ^ s.memsz).wrapping_mul(FNV_PRIME);
                h = (h ^ s.flags as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        match self.joint {
            Some(j) => h ^ (j as u64),
            None => h ^ 0x5a5a,
        }
    }

    /// 封册：根幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 基准固定址镜像：RX 文本 + 只读 + 读写三段。
pub fn sample_exec() -> FixedLoad {
    let mut l = FixedLoad::new();
    let _ = l.set_type(ET_EXEC);
    let _ = l.add_seg(0x400000, 0x2000, 0x2000, PF_R | PF_X);
    let _ = l.add_seg(0x402000, 0x1000, 0x1000, PF_R);
    let _ = l.add_seg(0x403000, 0x800, 0x1000, PF_R | PF_W);
    let _ = l.set_entry(0x401000);
    let _ = l.sign();
    l
}

/// 注入核：四路坏样本。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // (a) e_type 越闭集（ET_DYN）
    let mut a = FixedLoad::new();
    got[0] = a.set_type(3).err() == Some(EX_VER);
    // (b) 段重叠（同址重登）
    let mut b = FixedLoad::new();
    let _ = b.add_seg(0x400000, 0x1000, 0x1000, PF_R | PF_X);
    got[1] = b.add_seg(0x400000, 0x100, 0x100, PF_R).err() == Some(EX_SEG);
    // (c) 入口落在不可执行段
    let mut c = FixedLoad::new();
    let _ = c.add_seg(0x400000, 0x1000, 0x1000, PF_R | PF_X);
    let _ = c.add_seg(0x401000, 0x1000, 0x1000, PF_R | PF_W);
    got[2] = c.set_entry(0x401800).err() == Some(EX_EXEC);
    // (d) 封册后写账
    let mut d = sample_exec();
    let _ = d.freeze();
    got[3] = d.set_entry(0x401000).err() == Some(EX_FROZEN);
    got
}

/// 固定基址装载联签判据。
pub fn run_unx11263_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11263");

    // X1 双判据各自成立
    let x1 = {
        let l = sample_exec();
        l.loader_ok() && l.c1_ok()
    };
    s.add("X1-双判据成立", x1, "装载器判据与 C1 判据均为真");

    // X2 联签一致
    let x2 = {
        let mut l = sample_exec();
        let r = l.sign();
        r == Ok(true) && l.joint == Some(true)
    };
    s.add("X2-联签一致", x2, "两判据一致时联签为真");

    // X3 联签冲突立案
    let x3 = {
        let mut l = FixedLoad::new();
        let _ = l.add_seg(0x400000, 0x2000, 0x2000, PF_R | PF_X);
        let _ = l.add_seg(0x402000, 0x1000, 0x1000, PF_R);
        let _ = l.add_seg(0x403000, 0x800, 0x1000, PF_R | PF_W);
        let _ = l.set_entry(0x401000);
        let bad = l.sign().err() == Some(EX_JOINT);
        bad && l.joint == Some(false)
    };
    s.add("X3-联签冲突", x3, "缺 e_type 时装载器假/C1 真，冲突入账");

    // X4 指纹确定
    let x4 = {
        let l = sample_exec();
        let a = l.fold();
        a == l.fold() && a != 0
    };
    s.add("X4-指纹确定", x4, "同账两次指纹一致且非零");

    // X5 封册幂等
    let x5 = {
        let mut l = sample_exec();
        let r1 = l.freeze();
        let r2 = l.freeze();
        let live = l.fold();
        l.segs.clear();
        let r3 = l.freeze();
        r1 == r2 && r2 == r3 && r1.is_some() && live != l.fold()
    };
    s.add("X5-封册幂等", x5, "二次封册回原根，账变不改根");

    // X6 封册四路拒写
    let x6 = {
        let mut l = sample_exec();
        let _ = l.freeze();
        l.add_seg(0x410000, 0x1000, 0x1000, PF_R).err() == Some(EX_FROZEN)
            && l.set_type(ET_EXEC).err() == Some(EX_FROZEN)
            && l.set_entry(0x401000).err() == Some(EX_FROZEN)
            && l.sign().err() == Some(EX_FROZEN)
    };
    s.add("X6-封册拒写", x6, "段/类型/入口/联签四路均拒");

    // X7 页对齐门禁
    let x7 = {
        let mut l = FixedLoad::new();
        l.add_seg(0x400100, 0x10, 0x10, PF_R).err() == Some(EX_SEG)
    };
    s.add("X7-页对齐", x7, "未页对齐段地址报段错");

    // X8 固定窗口
    let x8 = {
        let mut a = FixedLoad::new();
        let low = a.add_seg(0x1000, 0x10, 0x10, PF_R).err() == Some(EX_BASE);
        let high = a.add_seg(0x0200_0000, 0x10, 0x10, PF_R).err() == Some(EX_BASE);
        low && high
    };
    s.add("X8-固定窗口", x8, "低于基址与高于窗口均报基址错");

    // X9 段重叠
    let x9 = {
        let mut l = FixedLoad::new();
        let _ = l.add_seg(0x400000, 0x2000, 0x2000, PF_R | PF_X);
        l.add_seg(0x401000, 0x100, 0x100, PF_R).err() == Some(EX_SEG)
    };
    s.add("X9-段重叠", x9, "区间相交报段重叠");

    // X10 类型闭集
    let x10 = {
        let mut a = FixedLoad::new();
        let dyn_ty = a.set_type(3).err() == Some(EX_VER);
        let none_ty = a.set_type(0).err() == Some(EX_VER);
        dyn_ty && none_ty
    };
    s.add("X10-类型闭集", x10, "ET_DYN 与 0 均非 ET_EXEC");

    // X11 入口可执行性
    let x11 = {
        let mut l = FixedLoad::new();
        let _ = l.add_seg(0x400000, 0x1000, 0x1000, PF_R | PF_X);
        let _ = l.add_seg(0x401000, 0x1000, 0x1000, PF_R | PF_W);
        let rw = l.set_entry(0x401800).err() == Some(EX_EXEC);
        let hole = l.set_entry(0x500000).err() == Some(EX_EXEC);
        rw && hole
    };
    s.add("X11-入口可执行", x11, "入口落 RW 段或落空段均拒");

    // X12 段表定容
    let x12 = {
        let mut l = FixedLoad::new();
        let mut i = 0u64;
        let mut ok = true;
        while i < SEG_CAP as u64 {
            if l.add_seg(0x400000u64.wrapping_add(i.wrapping_mul(0x1000)), 0x1000, 0x1000, PF_R)
                .is_err()
            {
                ok = false;
            }
            i = i.wrapping_add(1);
        }
        ok && l.add_seg(0x420000, 0x1000, 0x1000, PF_R).err() == Some(EX_CAP)
    };
    s.add("X12-段表定容", x12, "12 段后第 13 段报满");

    // X13 段尺寸形状与明细
    let (x13, line13) = {
        let mut l = FixedLoad::new();
        let big = l.add_seg(0x400000, 0x2000, 0x1000, PF_R).err() == Some(EX_SEG);
        let zero = l.add_seg(0x400000, 0, 0, PF_R).err() == Some(EX_SEG);
        let p = sample_exec();
        let line = format!("f>m={} zero={} first={}", big, zero, match p.segs.first() {
            Some(v) => v.render(),
            None => String::from("-"),
        });
        (big && zero && line.contains("RX@400000"), line)
    };
    s.add_dyn("X13-段尺寸形状", x13, &line13);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11263_flow() {
        let mut l = sample_exec();
        assert!(l.loader_ok());
        assert!(l.c1_ok());
        assert_eq!(l.joint, Some(true));
        assert_eq!(l.segs.len(), 3);
        assert_eq!(l.accepts, 6);
        assert_eq!(l.rejects, 0);
        let r1 = l.freeze();
        let r2 = l.freeze();
        assert_eq!(r1, r2);
        assert!(r1.is_some());
        assert!(l.root == r1);
        assert!(l.sign().err() == Some(EX_FROZEN));
        assert!(l.set_type(ET_EXEC).err() == Some(EX_FROZEN));
    }

    #[test]
    fn unx11263_gates() {
        let mut l = FixedLoad::new();
        assert!(l.set_type(ET_EXEC).is_ok());
        assert!(l.set_type(3).err() == Some(EX_VER));
        assert!(l.add_seg(0x400000, 0x2000, 0x2000, PF_R | PF_X).is_ok());
        assert!(l.add_seg(0x401000, 0x100, 0x100, PF_R).err() == Some(EX_SEG));
        assert!(l.add_seg(0x1234, 0x10, 0x10, PF_R).err() == Some(EX_SEG));
        assert!(l.add_seg(0x1000, 0x10, 0x10, PF_R).err() == Some(EX_BASE));
        assert!(l.add_seg(0x402000, 0x2000, 0x1000, PF_R).err() == Some(EX_SEG));
        assert!(l.add_seg(0x402000, 0x1000, 0x1000, PF_R).is_ok());
        assert!(l.add_seg(0x403000, 0x800, 0x1000, PF_R | PF_W).is_ok());
        assert!(l.set_entry(0x403800).err() == Some(EX_EXEC));
        assert!(l.set_entry(0x401000).is_ok());
        assert_eq!(l.sign(), Ok(true));
        assert!(l.freeze().is_some());
        assert!(l.add_seg(0x410000, 0x1000, 0x1000, PF_R).err() == Some(EX_FROZEN));
        assert!(l.set_type(ET_EXEC).err() == Some(EX_FROZEN));
        assert!(l.set_entry(0x401000).err() == Some(EX_FROZEN));
        assert!(l.sign().err() == Some(EX_FROZEN));
        assert_eq!(l.accepts, 6);
        assert_eq!(l.rejects, 10);
    }

    #[test]
    fn unx11263_checks_green() {
        assert!(run_unx11263_checks().all_passed(), "UNX-F11263 判据未全绿");
    }
}
