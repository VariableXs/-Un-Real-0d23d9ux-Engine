//! UNX-F11262 静态 PIE 装载路径（0x1f3e0 段）
//! 纯功能模型：ET_DYN 无 PT_INTERP 的静态 PIE——段表登记（LOAD/RELRO/BSS）、
//! 随机基址对齐选取、RELATIVE 重定位在 load_bias 上逐项落址；
//! 段重叠、基址未页对齐、重定位落空段三路坏样本各报自有码。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f3e0 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f3e0;
/// 段类别越出 1..=3 闭集。
pub const SP_KIND: u32 = SEG_BASE + 0x01;
/// 段重叠或同址重登。
pub const SP_SEG: u32 = SEG_BASE + 0x02;
/// 对齐违规（段对齐非 2 幂/小于页，或 vaddr 未对齐）。
pub const SP_ALIGN: u32 = SEG_BASE + 0x03;
/// 重定位落空段或未 8 字节对齐。
pub const SP_RELOC: u32 = SEG_BASE + 0x04;
/// 基址非法（零、未页对齐、撞栈区）。
pub const SP_BASE: u32 = SEG_BASE + 0x05;
/// 封册后写账。
pub const SP_FROZEN: u32 = SEG_BASE + 0x06;
/// 段表或重定位表满。
pub const SP_CAP: u32 = SEG_BASE + 0x07;

/// 段类别闭集。
pub const K_LOAD: u8 = 1;
pub const K_RELRO: u8 = 2;
pub const K_BSS: u8 = 3;
/// 页粒度（x86-64 4K，静态 PIE 最低 p_align）。
pub const PAGE: u64 = 0x1000;
/// 装载段下限（少于 4 段不算完整镜像）。
pub const SEG_NEED: usize = 4;
/// 段表定容。
pub const SEG_CAP: usize = 8;
/// 重定位表定容。
pub const REL_CAP: usize = 32;
/// 随机基址窗口。
pub const SPAN: u64 = 0x1000_0000;
/// 栈区下界（基址不得撞入）。
pub const STACK_LO: u64 = 0x7fff_0000_0000;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_62;

/// 单个装载段：仅记虚拟布局，文件偏移不参与语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PieSeg {
    pub kind: u8,
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

impl PieSeg {
    /// 段末（含 BSS 零填充）。
    pub fn end(&self) -> u64 {
        self.vaddr.wrapping_add(self.memsz)
    }

    /// 地址是否落在段内 [vaddr, end)。
    pub fn covers(&self, a: u64) -> bool {
        a >= self.vaddr && a < self.end()
    }

    /// 渲染：`L@400000+1234/2000 a1000`。
    pub fn render(&self) -> String {
        let tag = match self.kind {
            K_LOAD => "L",
            K_RELRO => "R",
            K_BSS => "B",
            _ => "?",
        };
        format!("{}@{:x}+{:x}/{:x} a{:x}", tag, self.vaddr, self.filesz, self.memsz, self.align)
    }
}

/// 静态 PIE 装载账：段表 → 基址 → 重定位落址。
#[derive(Clone, Debug)]
pub struct PieLoad {
    pub bias: u64,
    pub entry: u64,
    pub segs: Vec<PieSeg>,
    pub relocs: Vec<u64>,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl PieLoad {
    pub fn new() -> PieLoad {
        PieLoad {
            bias: 0,
            entry: 0,
            segs: Vec::new(),
            relocs: Vec::new(),
            accepts: 0,
            rejects: 0,
            frozen: false,
            root: None,
        }
    }

    /// 登记一段。判定序：封册 → 类别闭集 → 对齐 → 尺寸 → 重叠 → 定容。
    /// 重叠晚于尺寸：先保证单段自洽，再判段间关系。
    pub fn add_seg(&mut self, kind: u8, vaddr: u64, filesz: u64, memsz: u64, align: u64)
        -> Result<usize, u32>
    {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_FROZEN);
        }
        if kind < K_LOAD || kind > K_BSS {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_KIND);
        }
        if align < PAGE || align & align.wrapping_sub(1) != 0 || vaddr % align != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_ALIGN);
        }
        if filesz > memsz || memsz == 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_SEG);
        }
        let end = vaddr.wrapping_add(memsz);
        if end <= vaddr {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_SEG);
        }
        let mut k = 0usize;
        while k < self.segs.len() {
            if let Some(o) = self.segs.get(k) {
                if vaddr < o.end() && o.vaddr < end {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(SP_SEG);
                }
            }
            k = k.wrapping_add(1);
        }
        if self.segs.len() >= SEG_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_CAP);
        }
        self.segs.push(PieSeg { kind, vaddr, filesz, memsz, align });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.segs.len().wrapping_sub(1))
    }

    /// 段表最低虚拟地址（入口换算基准）。
    pub fn min_vaddr(&self) -> u64 {
        let mut m = u64::MAX;
        let mut k = 0usize;
        while k < self.segs.len() {
            if let Some(s) = self.segs.get(k) {
                if s.vaddr < m {
                    m = s.vaddr;
                }
            }
            k = k.wrapping_add(1);
        }
        if m == u64::MAX {
            0
        } else {
            m
        }
    }

    /// 设基址。判定序：封册 → 零/未页对齐/撞栈区 → 入口回绕。
    pub fn set_base(&mut self, hint: u64) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_FROZEN);
        }
        if hint == 0 || hint & (PAGE - 1) != 0 || hint >= STACK_LO.wrapping_sub(SPAN) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_BASE);
        }
        let e = hint.wrapping_add(self.min_vaddr());
        if e < hint {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_BASE);
        }
        self.bias = hint;
        self.entry = e;
        self.accepts = self.accepts.wrapping_add(1);
        Ok(hint)
    }

    /// 由种子派生随机基址（确定性，便于双机复算）。
    pub fn pick_base(&mut self, seed: u64) -> Result<u64, u32> {
        let r = seed.wrapping_mul(0x9e37_79b1_85eb_ca87) % SPAN;
        self.set_base(0x5555_0000u64.wrapping_add(r) & !(PAGE - 1))
    }

    /// RELATIVE 重定位落址：值 = bias + addend。
    pub fn relocate(&mut self, off: u64) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_FROZEN);
        }
        if off & 7 != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_RELOC);
        }
        let mut hit = false;
        let mut k = 0usize;
        while k < self.segs.len() {
            if let Some(s) = self.segs.get(k) {
                if s.covers(off) {
                    hit = true;
                }
            }
            k = k.wrapping_add(1);
        }
        if !hit || self.bias == 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_RELOC);
        }
        if self.relocs.len() >= REL_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SP_CAP);
        }
        let v = self.bias.wrapping_add(off);
        self.relocs.push(v);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(v)
    }

    /// 段类是否已登记。
    pub fn has_kind(&self, kind: u8) -> bool {
        self.segs.iter().any(|s| s.kind == kind)
    }

    /// 装载自洽：基址页对齐、入口落在基址之上、段类齐备、重定位全覆盖。
    pub fn verify(&self) -> bool {
        if self.bias == 0 || self.bias & (PAGE - 1) != 0 {
            return false;
        }
        if self.entry < self.bias || self.segs.len() < SEG_NEED {
            return false;
        }
        if !self.has_kind(K_LOAD) || !self.has_kind(K_RELRO) {
            return false;
        }
        let mut j = 0usize;
        while j < self.relocs.len() {
            if let Some(v) = self.relocs.get(j) {
                let off = v.wrapping_sub(self.bias);
                if off & 7 != 0 {
                    return false;
                }
                let mut cov = false;
                let mut k = 0usize;
                while k < self.segs.len() {
                    if let Some(s) = self.segs.get(k) {
                        if s.covers(off) {
                            cov = true;
                        }
                    }
                    k = k.wrapping_add(1);
                }
                if !cov {
                    return false;
                }
            }
            j = j.wrapping_add(1);
        }
        true
    }

    /// FNV-1a 链指纹：基址/入口/段序/重定位落址序敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.bias).wrapping_mul(FNV_PRIME);
        h = (h ^ self.entry).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.segs.len() {
            if let Some(s) = self.segs.get(k) {
                h = (h ^ s.kind as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.vaddr).wrapping_mul(FNV_PRIME);
                h = (h ^ s.filesz).wrapping_mul(FNV_PRIME);
                h = (h ^ s.memsz).wrapping_mul(FNV_PRIME);
                h = (h ^ s.align).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < self.relocs.len() {
            if let Some(v) = self.relocs.get(j) {
                h = (h ^ *v).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        h
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

/// 基准镜像：四段 + 三处重定位 + 随机基址。
pub fn sample_pie() -> PieLoad {
    let mut p = PieLoad::new();
    let _ = p.add_seg(K_LOAD, 0x0, 0x4000, 0x4000, PAGE);
    let _ = p.add_seg(K_LOAD, 0x4000, 0x1200, 0x1200, PAGE);
    let _ = p.add_seg(K_RELRO, 0x6000, 0x200, 0x200, PAGE);
    let _ = p.add_seg(K_BSS, 0x8000, 0x0, 0x1000, PAGE);
    let _ = p.set_base(0x5555_0000);
    let _ = p.relocate(0x10);
    let _ = p.relocate(0x4018);
    let _ = p.relocate(0x6010);
    p
}

/// 注入核：四路坏样本。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // (a) 段类别越闭集
    let mut a = PieLoad::new();
    got[0] = a.add_seg(9, 0, 16, 16, PAGE).err() == Some(SP_KIND);
    // (b) 段重叠（同址重登）
    let mut b = PieLoad::new();
    let _ = b.add_seg(K_LOAD, 0, 0x1000, 0x1000, PAGE);
    got[1] = b.add_seg(K_RELRO, 0, 0x100, 0x100, PAGE).err() == Some(SP_SEG);
    // (c) 对齐违规（vaddr 未按 p_align 对齐）
    let mut c = PieLoad::new();
    got[2] = c.add_seg(K_LOAD, 0x1234, 0x100, 0x100, PAGE).err() == Some(SP_ALIGN);
    // (d) 封册后写账
    let mut d = sample_pie();
    let _ = d.freeze();
    got[3] = d.relocate(0x10).err() == Some(SP_FROZEN);
    got
}

/// 静态 PIE 装载判据。
pub fn run_unx11262_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11262");

    // X1 基准镜像自洽
    let x1 = {
        let p = sample_pie();
        p.verify() && p.bias & (PAGE - 1) == 0 && p.segs.len() == SEG_NEED
    };
    s.add("X1-镜像自洽", x1, "四段齐备、基址页对齐、重定位全覆盖");

    // X2 入口换算
    let x2 = {
        let p = sample_pie();
        p.entry == p.bias.wrapping_add(p.min_vaddr()) && p.entry >= p.bias
    };
    s.add("X2-入口换算", x2, "入口 = 基址 + 最低段虚拟地址");

    // X3 重定位落址
    let x3 = {
        let p = sample_pie();
        p.relocs.len() == 3 && p.relocs.iter().all(|v| *v >= p.bias)
    };
    s.add("X3-重定位落址", x3, "三处相对重定位均落在基址之上");

    // X4 指纹确定
    let x4 = {
        let p = sample_pie();
        let a = p.fold();
        a == p.fold() && a != 0
    };
    s.add("X4-指纹确定", x4, "同镜像两次指纹一致且非零");

    // X5 封册幂等
    let x5 = {
        let mut p = sample_pie();
        let r1 = p.freeze();
        let r2 = p.freeze();
        let live = p.fold();
        p.segs.clear();
        let r3 = p.freeze();
        r1 == r2 && r2 == r3 && r1.is_some() && live != p.fold()
    };
    s.add("X5-封册幂等", x5, "二次封册回原根，账变不改根");

    // X6 封册拒写
    let x6 = {
        let mut p = sample_pie();
        let _ = p.freeze();
        p.add_seg(K_BSS, 0xa000, 0, 0x1000, PAGE).err() == Some(SP_FROZEN)
            && p.relocate(0x10).err() == Some(SP_FROZEN)
            && p.set_base(0x6000_0000).err() == Some(SP_FROZEN)
    };
    s.add("X6-封册拒写", x6, "段/重定位/基址三路均拒");

    // X7 段类别闭集
    let x7 = {
        let mut a = PieLoad::new();
        let low = a.add_seg(0, 0, 0x10, 0x10, PAGE).err() == Some(SP_KIND);
        let high = a.add_seg(4, 0, 0x10, 0x10, PAGE).err() == Some(SP_KIND);
        low && high
    };
    s.add("X7-段类别闭集", x7, "类别 0 与 4 均越闭集");

    // X8 段重叠
    let x8 = {
        let mut p = PieLoad::new();
        let _ = p.add_seg(K_LOAD, 0, 0x2000, 0x2000, PAGE);
        p.add_seg(K_RELRO, 0x1000, 0x100, 0x100, PAGE).err() == Some(SP_SEG)
    };
    s.add("X8-段重叠", x8, "同区间二次登记报重叠");

    // X9 对齐违规三路
    let x9 = {
        let mut a = PieLoad::new();
        let np = a.add_seg(K_LOAD, 0, 0x10, 0x10, 0x3000).err() == Some(SP_ALIGN);
        let small = a.add_seg(K_LOAD, 0, 0x10, 0x10, 0x100).err() == Some(SP_ALIGN);
        let off = a.add_seg(K_LOAD, 0x1234, 0x10, 0x10, PAGE).err() == Some(SP_ALIGN);
        np && small && off
    };
    s.add("X9-对齐违规", x9, "非 2 幂/小于页/未对齐三路均拒");

    // X10 基址门禁
    let x10 = {
        let mut a = PieLoad::new();
        let _ = a.add_seg(K_LOAD, 0, 0x1000, 0x1000, PAGE);
        let zero = a.set_base(0).err() == Some(SP_BASE);
        let mis = a.set_base(0x1234).err() == Some(SP_BASE);
        let stack = a.set_base(STACK_LO).err() == Some(SP_BASE);
        zero && mis && stack
    };
    s.add("X10-基址门禁", x10, "零/未对齐/撞栈区三路均拒");

    // X11 重定位门禁三路
    let x11 = {
        let mut a = PieLoad::new();
        let _ = a.add_seg(K_LOAD, 0, 0x1000, 0x1000, PAGE);
        let nobase = a.relocate(0x10).err() == Some(SP_RELOC);
        let _ = a.set_base(0x5555_0000);
        let unaligned = a.relocate(0x14).err() == Some(SP_RELOC);
        let outside = a.relocate(0x9000).err() == Some(SP_RELOC);
        nobase && unaligned && outside
    };
    s.add("X11-重定位门禁", x11, "无基址/未对齐/落空段均拒");

    // X12 重定位定容
    let x12 = {
        let mut p = PieLoad::new();
        let _ = p.add_seg(K_LOAD, 0, 0x1000, 0x1000, PAGE);
        let _ = p.set_base(0x5555_0000);
        let mut i = 0u64;
        while i < REL_CAP as u64 {
            let _ = p.relocate(i.wrapping_mul(8));
            i = i.wrapping_add(1);
        }
        p.relocate(0xf8).err() == Some(SP_CAP)
    };
    s.add("X12-重定位定容", x12, "32 条后第 33 条报满");

    // X13 段尺寸形状与明细
    let (x13, line13) = {
        let mut a = PieLoad::new();
        let big = a.add_seg(K_LOAD, 0, 0x2000, 0x1000, PAGE).err() == Some(SP_SEG);
        let zero = a.add_seg(K_LOAD, 0, 0, 0, PAGE).err() == Some(SP_SEG);
        let p = sample_pie();
        let line = format!("f>m={} zero={} seg0={}", big, zero, match p.segs.first() {
            Some(v) => v.render(),
            None => String::from("-"),
        });
        (big && zero && line.contains("L@0+4000"), line)
    };
    s.add_dyn("X13-段尺寸形状", x13, &line13);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11262_flow() {
        let mut p = sample_pie();
        assert!(p.verify());
        assert_eq!(p.bias, 0x5555_0000);
        assert_eq!(p.entry, 0x5555_0000);
        assert_eq!(p.segs.len(), 4);
        assert_eq!(p.relocs.len(), 3);
        assert_eq!(p.accepts, 8);
        assert_eq!(p.rejects, 0);
        let r1 = p.freeze();
        let r2 = p.freeze();
        assert_eq!(r1, r2);
        assert!(r1.is_some());
        assert!(p.root == r1);
        assert!(p.relocate(0x10).err() == Some(SP_FROZEN));
    }

    #[test]
    fn unx11262_gates() {
        let mut p = PieLoad::new();
        assert!(p.add_seg(K_LOAD, 0, 0x1000, 0x1000, PAGE).is_ok());
        assert!(p.add_seg(9, 0, 0x10, 0x10, PAGE).err() == Some(SP_KIND));
        assert!(p.add_seg(K_LOAD, 0x1234, 0x10, 0x10, PAGE).err() == Some(SP_ALIGN));
        assert!(p.add_seg(K_LOAD, 0x1000, 0x2000, 0x1000, PAGE).err() == Some(SP_SEG));
        assert!(p.add_seg(K_LOAD, 0x1000, 0x1000, 0x1000, PAGE).is_ok());
        assert!(p.add_seg(K_LOAD, 0, 0x100, 0x100, PAGE).err() == Some(SP_SEG));
        assert!(p.set_base(0).err() == Some(SP_BASE));
        assert!(p.set_base(0x5555_0000).is_ok());
        assert!(p.relocate(4).err() == Some(SP_RELOC));
        assert!(p.relocate(0x9000).err() == Some(SP_RELOC));
        assert!(p.relocate(0x10).is_ok());
        assert!(p.freeze().is_some());
        assert!(p.add_seg(K_BSS, 0x9000, 0, 0x1000, PAGE).err() == Some(SP_FROZEN));
        assert!(p.relocate(0x20).err() == Some(SP_FROZEN));
        assert!(p.set_base(0x6000_0000).err() == Some(SP_FROZEN));
        assert_eq!(p.accepts, 4);
        assert_eq!(p.rejects, 10);
    }

    #[test]
    fn unx11262_checks_green() {
        assert!(run_unx11262_checks().all_passed(), "UNX-F11262 判据未全绿");
    }
}
