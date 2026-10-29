//! UNX-F11286 dladdr 反查语义（0x1f560 段）
//!
//! 纯功能模型：`dladdr(addr)` 把运行期地址反查成（库名 / 符号名 / 符号起址 /
//! 库内偏移）。本模块把装载对象的地址区间与符号表分开记：对象区间是半开区间
//! [base+lo, base+hi)，符号命中条件为 start <= addr < start+size；**尺寸为零
//! 的符号只在其起址精确命中**（glibc 对 STT_NOTYPE/size=0 的行为），因此
//! 边界地址 addr == start+size 恒不命中本符号而落到后继符号或「无符号」。
//!
//! 规则面：地址零直接判错（dladdr 的 NULL 反查无意义）；区间外地址返回"无
//! 库"而不是错误；同对象内多个符号重叠时取起址最大者（最具体）；跨对象同
//! 地址互不串——先定对象再定符号；无符号命中时仍回报库名并把 saddr 置零。
//!
//! 拒止面：DA_EADDR（零地址反查）、DA_ENAME（名号为零）、DA_EID（对象号
//! 越界）、DA_EOBJ（符号所属对象未登记）、DA_ERANGE（符号起址落在对象
//! 区间外）、DA_EDUP（同对象同起址重号）、DA_ECAP（表满）、DA_EFROZEN
//! （冻结后登记）、DA_EMISMATCH（重放指纹不一致）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x1f560 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f560;

/// 零地址反查
pub const DA_EADDR: u32 = SEG_BASE + 0x01;
/// 名号为零
pub const DA_ENAME: u32 = SEG_BASE + 0x02;
/// 对象号越界
pub const DA_EID: u32 = SEG_BASE + 0x03;
/// 符号所属对象未登记
pub const DA_EOBJ: u32 = SEG_BASE + 0x04;
/// 符号起址落在对象区间外
pub const DA_ERANGE: u32 = SEG_BASE + 0x05;
/// 同对象同起址重号
pub const DA_EDUP: u32 = SEG_BASE + 0x06;
/// 表满
pub const DA_ECAP: u32 = SEG_BASE + 0x07;
/// 冻结后登记
pub const DA_EFROZEN: u32 = SEG_BASE + 0x08;
/// 重放指纹不一致
pub const DA_EMISMATCH: u32 = SEG_BASE + 0x09;

/// 对象上限。
pub const MAX_OBJS: usize = 4;
/// 符号上限。
pub const MAX_SYMS: usize = 16;
/// 无对象。
pub const NO_OBJ: u8 = 0xff;
/// 无符号。
pub const NO_SYM: u8 = 0xff;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个装载对象的地址区间（半开）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjRange {
    pub name_id: u16,
    pub lo: u64,
    pub hi: u64,
}

impl ObjRange {
    /// 是否含该地址（半开区间）。
    pub fn holds(&self, addr: u64) -> bool {
        addr >= self.lo && addr < self.hi
    }
}

/// 一个符号：名号 / 起址 / 尺寸 / 所属对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sym {
    pub name_id: u16,
    pub start: u64,
    pub size: u32,
    pub obj: u8,
}

impl Sym {
    /// 是否命中该地址：尺寸零只认精确起址。
    pub fn holds(&self, addr: u64) -> bool {
        if self.size == 0 {
            return addr == self.start;
        }
        addr >= self.start && addr < self.start.wrapping_add(self.size as u64)
    }
}

/// 反查结论：对象下标 / 符号下标（可 NO_SYM）/ 库内偏移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DlInfo {
    pub obj: u8,
    pub sym: u8,
    pub off: u64,
}

/// dladdr 反查表。
#[derive(Clone, Debug)]
pub struct DlTable {
    objs: [Option<ObjRange>; MAX_OBJS],
    n_obj: usize,
    syms: [Option<Sym>; MAX_SYMS],
    n_sym: usize,
    frozen: bool,
    root: Option<u64>,
    pub accepts: u64,
    pub rejects: u64,
    pub lookups: u64,
    pub hits: u64,
    pub misses: u64,
}

impl DlTable {
    /// 空表。
    pub fn new() -> DlTable {
        DlTable {
            objs: [None; MAX_OBJS],
            n_obj: 0,
            syms: [None; MAX_SYMS],
            n_sym: 0,
            frozen: false,
            root: None,
            accepts: 0,
            rejects: 0,
            lookups: 0,
            hits: 0,
            misses: 0,
        }
    }

    fn note(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 对象数。
    pub fn obj_count(&self) -> usize {
        self.n_obj
    }

    /// 符号数。
    pub fn sym_count(&self) -> usize {
        self.n_sym
    }

    /// 取对象。
    pub fn obj(&self, i: usize) -> Option<ObjRange> {
        match self.objs.get(i) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 取符号。
    pub fn sym(&self, i: usize) -> Option<Sym> {
        match self.syms.get(i) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 登记一个对象区间。校验序：冻结 → 名号 → 区间 → 重名 → 容量。
    pub fn add_object(&mut self, name_id: u16, lo: u64, hi: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.note(DA_EFROZEN));
        }
        if name_id == 0 {
            return Err(self.note(DA_ENAME));
        }
        if lo >= hi {
            return Err(self.note(DA_ERANGE));
        }
        if self.obj_of_name(name_id).is_some() {
            return Err(self.note(DA_EDUP));
        }
        if self.n_obj >= MAX_OBJS {
            return Err(self.note(DA_ECAP));
        }
        match self.objs.get_mut(self.n_obj) {
            Some(cell) => *cell = Some(ObjRange { name_id, lo, hi }),
            None => return Err(self.note(DA_ECAP)),
        }
        self.n_obj = self.n_obj.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 按名号找对象下标。
    pub fn obj_of_name(&self, name_id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n_obj {
            match self.objs.get(i) {
                Some(Some(o)) if o.name_id == name_id => return Some(i),
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 登一个符号。校验序：冻结 → 对象号 → 名号 → 起址区间 → 重号 → 容量。
    pub fn add_symbol(&mut self, obj: u8, name_id: u16, start: u64, size: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.note(DA_EFROZEN));
        }
        if obj as usize >= self.n_obj {
            return Err(self.note(DA_EID));
        }
        if name_id == 0 {
            return Err(self.note(DA_ENAME));
        }
        let range = match self.objs.get(obj as usize) {
            Some(Some(r)) => *r,
            _ => return Err(self.note(DA_EOBJ)),
        };
        if start < range.lo || start >= range.hi {
            return Err(self.note(DA_ERANGE));
        }
        let mut i = 0usize;
        while i < self.n_sym {
            match self.syms.get(i) {
                Some(Some(s)) => {
                    if s.obj == obj && s.start == start {
                        return Err(self.note(DA_EDUP));
                    }
                }
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        if self.n_sym >= MAX_SYMS {
            return Err(self.note(DA_ECAP));
        }
        match self.syms.get_mut(self.n_sym) {
            Some(cell) => *cell = Some(Sym { name_id, start, size, obj }),
            None => return Err(self.note(DA_ECAP)),
        }
        self.n_sym = self.n_sym.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 反查：先定对象，再定符号（同对象重叠取起址最大者）。
    pub fn lookup(&mut self, addr: u64) -> Result<Option<DlInfo>, u32> {
        if addr == 0 {
            return Err(self.note(DA_EADDR));
        }
        self.lookups = self.lookups.wrapping_add(1);
        let mut obj_i = NO_OBJ;
        let mut i = 0usize;
        while i < self.n_obj {
            match self.objs.get(i) {
                Some(Some(r)) => {
                    if r.holds(addr) && obj_i == NO_OBJ {
                        obj_i = i as u8;
                    }
                }
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        if obj_i == NO_OBJ {
            self.misses = self.misses.wrapping_add(1);
            return Ok(None);
        }
        let mut best = NO_SYM;
        let mut best_start = 0u64;
        let mut k = 0usize;
        while k < self.n_sym {
            match self.syms.get(k) {
                Some(Some(s)) => {
                    if s.obj == obj_i && s.holds(addr) && (best == NO_SYM || s.start >= best_start) {
                        best = k as u8;
                        best_start = s.start;
                    }
                }
                _ => {}
            }
            k = k.wrapping_add(1);
        }
        self.hits = self.hits.wrapping_add(1);
        let base = match self.objs.get(obj_i as usize) {
            Some(Some(r)) => r.lo,
            _ => 0,
        };
        if best == NO_SYM {
            return Ok(Some(DlInfo { obj: obj_i, sym: NO_SYM, off: addr.wrapping_sub(base) }));
        }
        Ok(Some(DlInfo { obj: obj_i, sym: best, off: addr.wrapping_sub(best_start) }))
    }

    /// 表指纹（对象区间 + 符号表）。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n_obj {
            match self.objs.get(i) {
                Some(Some(r)) => {
                    h = (h ^ r.name_id as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ r.lo).wrapping_mul(FNV_PRIME);
                    h = (h ^ r.hi).wrapping_mul(FNV_PRIME);
                }
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        let mut k = 0usize;
        while k < self.n_sym {
            match self.syms.get(k) {
                Some(Some(s)) => {
                    h = (h ^ s.name_id as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ s.start).wrapping_mul(FNV_PRIME);
                    h = (h ^ s.size as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ s.obj as u64).wrapping_mul(FNV_PRIME);
                }
                _ => {}
            }
            k = k.wrapping_add(1);
        }
        h = (h ^ self.n_obj as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.n_sym as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结（幂等），空表拒。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return match self.root {
                Some(r) => Ok(r),
                None => Ok(self.fold()),
            };
        }
        if self.n_sym == 0 {
            return Err(self.note(DA_EOBJ));
        }
        let v = self.fold();
        self.root = Some(v);
        self.frozen = true;
        Ok(v)
    }

    /// 定锚值。
    pub fn root_value(&self) -> Option<u64> {
        self.root
    }

    /// 重放：重算指纹并与定锚比对。
    pub fn replay(&mut self) -> Result<u64, u32> {
        let root = match self.root {
            Some(r) => r,
            None => return Err(self.note(DA_EMISMATCH)),
        };
        if self.fold() != root {
            return Err(self.note(DA_EMISMATCH));
        }
        Ok(root)
    }
}

/// 基准表：两个对象、四个符号（含一个零尺寸符号）。
pub fn build_table() -> (DlTable, bool) {
    let mut t = DlTable::new();
    let a = t.add_object(1, 0x1000, 0x2000).is_ok();
    let b = t.add_object(2, 0x5000, 0x6000).is_ok();
    let c = t.add_symbol(0, 10, 0x1000, 0x40).is_ok();
    let d = t.add_symbol(0, 11, 0x1040, 0x20).is_ok();
    let e = t.add_symbol(0, 12, 0x1100, 0).is_ok();
    let f = t.add_symbol(1, 20, 0x5000, 0x100).is_ok();
    let g = t.freeze().is_ok();
    (t, a && b && c && d && e && f && g)
}

/// 四类注入：零地址 / 名号与区间 / 重号 / 冻结与满席。
pub fn inject_four() -> [bool; 4] {
    // ① 零地址反查拒
    let mut a0 = DlTable::new();
    let _ = a0.add_object(1, 0x1000, 0x2000);
    let r0 = a0.lookup(0).err() == Some(DA_EADDR);

    // ② 名号为零 / 区间反向 / 符号越界
    let mut a1 = DlTable::new();
    let r1 = a1.add_object(0, 0x1000, 0x2000).err() == Some(DA_ENAME)
        && a1.add_object(1, 0x2000, 0x1000).err() == Some(DA_ERANGE)
        && a1.add_object(1, 0x1000, 0x2000).is_ok()
        && a1.add_symbol(7, 10, 0x1000, 8).err() == Some(DA_EID)
        && a1.add_symbol(0, 10, 0x3000, 8).err() == Some(DA_ERANGE);

    // ③ 重号：对象名重、符号起址重
    let mut a2 = DlTable::new();
    let _ = a2.add_object(1, 0x1000, 0x2000);
    let _ = a2.add_symbol(0, 10, 0x1000, 0x10);
    let r2 = a2.add_object(1, 0x3000, 0x4000).err() == Some(DA_EDUP)
        && a2.add_symbol(0, 11, 0x1000, 0x20).err() == Some(DA_EDUP);

    // ④ 冻结后登记 / 表满
    let mut a3 = DlTable::new();
    let _ = a3.add_object(1, 0x1000, 0x2000);
    let _ = a3.add_symbol(0, 10, 0x1000, 0x10);
    let frozen = a3.freeze().is_ok();
    let blocked = a3.add_symbol(0, 11, 0x1010, 0x10).err() == Some(DA_EFROZEN);
    let mut full = DlTable::new();
    let _ = full.add_object(1, 0x1000, 0x10000);
    let mut i = 0u64;
    while i < MAX_SYMS as u64 {
        let _ = full.add_symbol(0, 100u16.wrapping_add(i as u16), 0x1000 + i * 4, 4);
        i = i.wrapping_add(1);
    }
    let cap = full.add_symbol(0, 999, 0x2000, 4).err() == Some(DA_ECAP);
    let r3 = frozen && blocked && cap && full.sym_count() == MAX_SYMS;

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11286 dladdr 反查语义。
pub fn run_unx11286_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11286.dladdr_lookup");
    let (mut t, ok) = build_table();

    // DA1 区间内命中
    let h1 = t.lookup(0x1010).ok().flatten();
    s.add_dyn(
        "DA1-区间内命中",
        ok && h1 == Some(DlInfo { obj: 0, sym: 0, off: 0x10 }),
        &format!("h={:?}", h1),
    );

    // DA2 区间尾端排斥（end exclusive）
    let h2a = t.lookup(0x1040).ok().flatten();
    let h2b = t.lookup(0x1060).ok().flatten();
    s.add_dyn(
        "DA2-尾端排斥",
        h2a.map(|h| h.sym) == Some(1) && h2a.map(|h| h.off) == Some(0)
            && h2b.map(|h| h.sym) == Some(NO_SYM),
        &format!("a={:?} b={:?}", h2a, h2b),
    );

    // DA3 零尺寸符号只精确命中
    let h3a = t.lookup(0x1100).ok().flatten();
    let h3b = t.lookup(0x1101).ok().flatten();
    s.add(
        "DA3-零尺寸精确",
        h3a.map(|h| h.sym) == Some(2) && h3a.map(|h| h.off) == Some(0)
            && h3b.map(|h| h.sym) == Some(NO_SYM),
        "size=0 仅起址命中，起址+1 即为无符号",
    );

    // DA4 区间外无库
    let h4 = t.lookup(0x300).ok().flatten();
    s.add(
        "DA4-区间外无库",
        h4.is_none() && t.misses == 1,
        "区间外地址返回 None 而非错误",
    );

    // DA5 零地址拒
    let mut z = DlTable::new();
    s.add(
        "DA5-零地址拒查",
        z.lookup(0).err() == Some(DA_EADDR) && z.lookups == 0,
        "NULL 反查直接拒且不计反查数",
    );

    // DA6 偏移计算（库内偏移与符号内偏移）
    let h6 = t.lookup(0x5008).ok().flatten();
    s.add_dyn(
        "DA6-偏移计算",
        h6 == Some(DlInfo { obj: 1, sym: 3, off: 8 }),
        &format!("h={:?}", h6),
    );

    // DA7 重叠符号取起址最大者
    let mut o = DlTable::new();
    let _ = o.add_object(1, 0x1000, 0x2000);
    let _ = o.add_symbol(0, 30, 0x1000, 0x100);
    let _ = o.add_symbol(0, 31, 0x1030, 0x40);
    let h7 = o.lookup(0x1035).ok().flatten();
    s.add_dyn(
        "DA7-重叠取最具体",
        h7.map(|h| h.sym) == Some(1) && h7.map(|h| h.off) == Some(5),
        &format!("h={:?}", h7),
    );

    // DA8 跨对象隔离
    let h8a = t.lookup(0x5000).ok().flatten();
    let h8b = t.lookup(0x1000).ok().flatten();
    s.add(
        "DA8-跨对象隔离",
        h8a.map(|h| h.obj) == Some(1) && h8a.map(|h| h.sym) == Some(3)
            && h8b.map(|h| h.obj) == Some(0) && h8b.map(|h| h.sym) == Some(0),
        "同偏移地址按对象区间各自命中",
    );

    // DA9 有库无符号
    let h9 = t.lookup(0x1060).ok().flatten();
    s.add_dyn(
        "DA9-有库无符号",
        h9 == Some(DlInfo { obj: 0, sym: NO_SYM, off: 0x60 }),
        &format!("h={:?}", h9),
    );

    // DA10 注册拒止
    let mut d = DlTable::new();
    let d1 = d.add_object(0, 0x1000, 0x2000).err() == Some(DA_ENAME);
    let d2 = d.add_object(1, 0x2000, 0x2000).err() == Some(DA_ERANGE);
    let d3 = d.add_object(1, 0x1000, 0x2000).is_ok();
    let d4 = d.add_object(1, 0x3000, 0x4000).err() == Some(DA_EDUP);
    let d5 = d.add_symbol(0, 10, 0x2fff, 0x10).err() == Some(DA_ERANGE);
    let d6 = d.add_symbol(0, 0, 0x1000, 0x10).err() == Some(DA_ENAME);
    s.add_dyn(
        "DA10-注册拒止",
        d1 && d2 && d3 && d4 && d5 && d6 && d.obj_count() == 1,
        &format!("obj={} rej={}", d.obj_count(), d.rejects),
    );

    // DA11 冻结幂等 + 冻结后拒
    let f1 = t.freeze().unwrap_or(0);
    let f2 = t.freeze().unwrap_or(0);
    s.add_dyn(
        "DA11-冻结幂等",
        f1 == f2 && f1 != 0
            && t.add_object(3, 0x9000, 0xa000).err() == Some(DA_EFROZEN)
            && t.add_symbol(0, 99, 0x1200, 8).err() == Some(DA_EFROZEN),
        &format!("f={:#x}", f1),
    );

    // DA12 重放
    let rep = t.replay().unwrap_or(0);
    s.add_dyn(
        "DA12-重放同指纹",
        rep == f1 && t.root_value() == Some(f1) && t.hits >= 5,
        &format!("rep={:#x} hits={}", rep, t.hits),
    );

    // DA13 注入四拒
    let hits = inject_four();
    s.add(
        "DA13-注入四拒",
        hits[0] && hits[1] && hits[2] && hits[3],
        "零地址/名号区间/重号/冻结满席四路皆拒",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11286_dladdr_flow() {
        let (mut t, ok) = build_table();
        assert!(ok);
        assert_eq!(t.obj_count(), 2);
        assert_eq!(t.sym_count(), 4);
        assert_eq!(t.lookup(0x1000).ok().flatten(), Some(DlInfo { obj: 0, sym: 0, off: 0 }));
        assert_eq!(t.lookup(0x1040).ok().flatten(), Some(DlInfo { obj: 0, sym: 1, off: 0 }));
        assert_eq!(
            t.lookup(0x1050).ok().flatten(),
            Some(DlInfo { obj: 0, sym: 1, off: 0x10 })
        );
        assert_eq!(
            t.lookup(0x1070).ok().flatten(),
            Some(DlInfo { obj: 0, sym: NO_SYM, off: 0x70 })
        );
        assert_eq!(t.lookup(0x1100).ok().flatten(), Some(DlInfo { obj: 0, sym: 2, off: 0 }));
        assert!(t.lookup(0x9000).ok().flatten().is_none());
        let root = t.root_value();
        assert_eq!(t.freeze().ok(), root);
        assert_eq!(t.replay().ok(), root);
    }

    #[test]
    fn f11286_dladdr_gates() {
        let mut t = DlTable::new();
        assert_eq!(t.add_object(0, 0x1000, 0x2000), Err(DA_ENAME));
        assert_eq!(t.add_object(1, 0x2000, 0x1000), Err(DA_ERANGE));
        assert!(t.add_object(1, 0x1000, 0x2000).is_ok());
        assert_eq!(t.add_object(1, 0x3000, 0x4000), Err(DA_EDUP));
        assert_eq!(t.add_symbol(7, 10, 0x1000, 8), Err(DA_EID));
        assert_eq!(t.add_symbol(0, 10, 0x3000, 8), Err(DA_ERANGE));
        assert_eq!(t.add_symbol(0, 0, 0x1000, 8), Err(DA_ENAME));
        assert!(t.add_symbol(0, 10, 0x1000, 8).is_ok());
        assert_eq!(t.add_symbol(0, 11, 0x1000, 0x10), Err(DA_EDUP));
        assert_eq!(t.lookup(0), Err(DA_EADDR));
        assert_eq!(t.rejects, 8);
        assert_eq!(t.accepts, 2);
        assert!(t.freeze().is_ok());
    }

    #[test]
    fn f11286_dladdr_rules() {
        let (mut t, _) = build_table();
        // 精确边界：符号尾端落到后继符号
        assert_eq!(t.lookup(0x103f).ok().flatten().map(|h| h.sym), Some(0));
        assert_eq!(t.lookup(0x1040).ok().flatten().map(|h| h.sym), Some(1));
        assert_eq!(t.lookup(0x1060).ok().flatten().map(|h| h.sym), Some(NO_SYM));
        // 零尺寸符号
        assert_eq!(t.lookup(0x1100).ok().flatten().map(|h| h.sym), Some(2));
        assert_eq!(t.lookup(0x10ff).ok().flatten().map(|h| h.sym), Some(NO_SYM));
        // 对象尾端排斥
        assert!(t.lookup(0x2000).ok().flatten().is_none());
        assert!(t.lookup(0x1fff).ok().flatten().is_some());
        // 空表冻结拒
        let mut e = DlTable::new();
        assert_eq!(e.freeze(), Err(DA_EOBJ));
    }

    #[test]
    fn f11286_dladdr_injection() {
        let hits = inject_four();
        assert!(hits[0] && hits[1] && hits[2] && hits[3]);
    }

    #[test]
    fn f11286_checks_green() {
        let cs = run_unx11286_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
    }
}
