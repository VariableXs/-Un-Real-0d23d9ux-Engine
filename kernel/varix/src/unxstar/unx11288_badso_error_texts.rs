//! UNX-F11288 坏 so 装载错误文本族（0x1f580 段）
//!
//! 纯功能模型：把「坏共享对象」拆成六类——空文件、截断、错类（ELFCLASS32）、
//! 错机（EM_AARCH64）、坏段（p_vaddr 与 p_offset 不同余于页）、坏依赖（DT_NEEDED
//! 目标在搜索路径内不可解析）。装载器先按固定优先序判定类，再产出该类**逐字
//! 固定**的错误文本与退出码：除正常装载外一律 exit 127，文本与 Linux 参考机
//! 一致（如 "wrong ELF class: ELFCLASS32"、"file too short"）。
//!
//! 规则面：判定优先序为 尺寸归零 → 头部截断 → 魔数 → 类别 → 机器 → 段同余 →
//! 依赖解析。前置类错误永不因为后续字段更"像样"而被覆盖——这保证同一个坏件
//! 永远报同一条文本（诊断可比）。依赖类文本要把 soname 拼在文本之前，拼法
//! 固定为 "<soname>: <text>"，故逐字比对须整串比而不是子串比。
//!
//! 拒止面：SO_ENAME（对象名号为零）、SO_EBADID（名号越界）、SO_EDUP（同名
//! 已成功装载再登）、SO_ECAP（装载账满）、SO_EFROZEN（冻结后登账）、
//! SO_ERESOLVER（解析表非升序）、SO_EMISMATCH（重放指纹不一致）、
//! SO_EEMPTY（空账即封）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x1f580 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f580;

/// 对象名号为零
pub const SO_ENAME: u32 = SEG_BASE + 0x01;
/// 名号越界
pub const SO_EBADID: u32 = SEG_BASE + 0x02;
/// 同名已成功装载再登
pub const SO_EDUP: u32 = SEG_BASE + 0x03;
/// 装载账满
pub const SO_ECAP: u32 = SEG_BASE + 0x04;
/// 冻结后登账
pub const SO_EFROZEN: u32 = SEG_BASE + 0x05;
/// 解析表非升序
pub const SO_ERESOLVER: u32 = SEG_BASE + 0x06;
/// 重放指纹不一致
pub const SO_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 空账即封
pub const SO_EEMPTY: u32 = SEG_BASE + 0x08;

/// 装载账席位。
pub const MAX_OBJS: usize = 16;
/// 单个对象 DT_NEEDED 上限。
pub const MAX_NEEDS: usize = 4;
/// 解析表容量。
pub const MAX_RESOLVED: usize = 8;
/// ELF 头长度（64 位）。
pub const EHDR_SIZE: u32 = 64;
/// ELF 魔数（小端读作 u32）。
pub const ELF_MAGIC: u32 = 0x464c_457f;
/// ELFCLASS64。
pub const ELFCLASS64: u8 = 2;
/// ELFCLASS32。
pub const ELFCLASS32: u8 = 1;
/// EM_X86_64。
pub const EM_X86_64: u16 = 62;
/// EM_AARCH64。
pub const EM_AARCH64: u16 = 183;
/// 页大小。
pub const PAGE: u64 = 4096;
/// 坏件统一退出码。
pub const EXIT_BAD: u8 = 127;
/// 正常退出码。
pub const EXIT_OK: u8 = 0;

/// 判定类：正常。
pub const V_OK: u8 = 0;
/// 判定类：空文件。
pub const V_EMPTY: u8 = 1;
/// 判定类：截断。
pub const V_TRUNC: u8 = 2;
/// 判定类：错类。
pub const V_CLASS: u8 = 3;
/// 判定类：错机。
pub const V_MACH: u8 = 4;
/// 判定类：坏段。
pub const V_SEG: u8 = 5;
/// 判定类：坏依赖。
pub const V_DEP: u8 = 6;

/// 文本：空文件。
pub const TEXT_EMPTY: &str = "invalid ELF header: empty file";
/// 文本：截断。
pub const TEXT_TRUNC: &str = "file too short";
/// 文本：错类。
pub const TEXT_CLASS: &str = "wrong ELF class: ELFCLASS32";
/// 文本：错机。
pub const TEXT_MACH: &str = "wrong ELF machine: EM_AARCH64";
/// 文本：坏段。
pub const TEXT_SEG: &str = "cannot map segment: unaligned p_vaddr";
/// 文本：坏依赖。
pub const TEXT_DEP: &str = "cannot open shared object file: No such file or directory";
/// 文本：正常。
pub const TEXT_OK: &str = "loaded";

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// ELF 头部镜像（只保留判定所需字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElfMeta {
    pub name_id: u16,
    pub size: u32,
    pub magic: u32,
    pub class: u8,
    pub machine: u16,
    pub phnum: u16,
    pub vaddr: u64,
    pub offset: u64,
    pub needs: [u16; MAX_NEEDS],
    pub need_n: usize,
}

/// 解析表：搜索路径内可解析的 soname 名号（升序）。
#[derive(Clone, Copy, Debug)]
pub struct Resolver {
    ids: [u16; MAX_RESOLVED],
    n: usize,
}

impl Resolver {
    /// 空表。
    pub fn new() -> Resolver {
        Resolver { ids: [0; MAX_RESOLVED], n: 0 }
    }

    /// 增补一个可解析名号（调用方按升序登记）。
    pub fn push(&mut self, id: u16) -> bool {
        if self.n >= MAX_RESOLVED || id == 0 {
            return false;
        }
        match self.ids.get_mut(self.n) {
            Some(c) => {
                *c = id;
                self.n = self.n.wrapping_add(1);
                true
            }
            None => false,
        }
    }

    /// 表内是否含该名号。
    pub fn contains(&self, id: u16) -> bool {
        let mut i = 0usize;
        while i < self.n {
            match self.ids.get(i) {
                Some(v) if *v == id => return true,
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        false
    }

    /// 表是否严格升序（诊断前置条件）。
    pub fn is_sorted(&self) -> bool {
        let mut i = 1usize;
        while i < self.n {
            let a = match self.ids.get(i.wrapping_sub(1)) {
                Some(v) => *v,
                None => return false,
            };
            let b = match self.ids.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a >= b {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 表内条目数。
    pub fn len(&self) -> usize {
        self.n
    }
}

/// 一次判定的结论：类 / 文本 / 退出码 / 首个缺失依赖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub class: u8,
    pub text: &'static str,
    pub exit: u8,
    pub missing: u16,
}

impl Verdict {
    /// 是否装载成功。
    pub fn ok(&self) -> bool {
        self.class == V_OK
    }
}

/// 六类判定（优先序即函数体顺序，前置类不被后续覆盖）。
pub fn diagnose(m: &ElfMeta, r: &Resolver) -> Verdict {
    if m.size == 0 {
        return Verdict { class: V_EMPTY, text: TEXT_EMPTY, exit: EXIT_BAD, missing: 0 };
    }
    if m.size < EHDR_SIZE {
        return Verdict { class: V_TRUNC, text: TEXT_TRUNC, exit: EXIT_BAD, missing: 0 };
    }
    if m.magic != ELF_MAGIC {
        return Verdict { class: V_EMPTY, text: TEXT_EMPTY, exit: EXIT_BAD, missing: 0 };
    }
    if m.class != ELFCLASS64 {
        return Verdict { class: V_CLASS, text: TEXT_CLASS, exit: EXIT_BAD, missing: 0 };
    }
    if m.machine != EM_X86_64 {
        return Verdict { class: V_MACH, text: TEXT_MACH, exit: EXIT_BAD, missing: 0 };
    }
    if m.vaddr % PAGE != m.offset % PAGE {
        return Verdict { class: V_SEG, text: TEXT_SEG, exit: EXIT_BAD, missing: 0 };
    }
    let mut i = 0usize;
    while i < m.need_n {
        match m.needs.get(i) {
            Some(id) => {
                if !r.contains(*id) {
                    return Verdict { class: V_DEP, text: TEXT_DEP, exit: EXIT_BAD, missing: *id };
                }
            }
            None => break,
        }
        i = i.wrapping_add(1);
    }
    Verdict { class: V_OK, text: TEXT_OK, exit: EXIT_OK, missing: 0 }
}

/// 依赖类整串渲染："<soname>: <text>"；其余类原样返回文本。
pub fn render(v: &Verdict, soname: &str) -> alloc::string::String {
    if v.class == V_DEP {
        format!("{}: {}", soname, v.text)
    } else {
        format!("{}", v.text)
    }
}

/// 一条装载账：名号 / 判定类 / 退出码 / 缺失依赖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name_id: u16,
    pub class: u8,
    pub exit: u8,
    pub missing: u16,
}

/// 装载账。
#[derive(Clone, Debug)]
pub struct SoLedger {
    entries: [Option<Entry>; MAX_OBJS],
    n: usize,
    per_class: [u32; 7],
    root: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
    pub loaded: u64,
    pub failed: u64,
}

impl SoLedger {
    /// 空账。
    pub fn new() -> SoLedger {
        SoLedger {
            entries: [None; MAX_OBJS],
            n: 0,
            per_class: [0; 7],
            root: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
            loaded: 0,
            failed: 0,
        }
    }

    fn note(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已登条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 某判定类的条数。
    pub fn class_count(&self, c: u8) -> u32 {
        match self.per_class.get(c as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 取一条。
    pub fn entry(&self, i: usize) -> Option<Entry> {
        match self.entries.get(i) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 同名成功装载是否已存在。
    fn loaded_name(&self, id: u16) -> bool {
        let mut i = 0usize;
        while i < self.n {
            match self.entries.get(i) {
                Some(Some(e)) => {
                    if e.name_id == id && e.class == V_OK {
                        return true;
                    }
                }
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        false
    }

    /// 登一次装载。校验序：冻结 → 名号 → 表序 → 席位 → 重载。
    pub fn record(&mut self, m: &ElfMeta, r: &Resolver) -> Result<Verdict, u32> {
        if self.frozen {
            return Err(self.note(SO_EFROZEN));
        }
        if m.name_id == 0 {
            return Err(self.note(SO_ENAME));
        }
        if m.name_id > 0x0fff {
            return Err(self.note(SO_EBADID));
        }
        if !r.is_sorted() {
            return Err(self.note(SO_ERESOLVER));
        }
        if self.n >= MAX_OBJS {
            return Err(self.note(SO_ECAP));
        }
        if self.loaded_name(m.name_id) {
            return Err(self.note(SO_EDUP));
        }
        let v = diagnose(m, r);
        let e = Entry { name_id: m.name_id, class: v.class, exit: v.exit, missing: v.missing };
        match self.entries.get_mut(self.n) {
            Some(cell) => *cell = Some(e),
            None => return Err(self.note(SO_ECAP)),
        }
        self.n = self.n.wrapping_add(1);
        let slot = v.class as usize;
        if let Some(c) = self.per_class.get_mut(slot) {
            *c = c.wrapping_add(1);
        }
        if v.class == V_OK {
            self.loaded = self.loaded.wrapping_add(1);
        } else {
            self.failed = self.failed.wrapping_add(1);
        }
        self.accepts = self.accepts.wrapping_add(1);
        Ok(v)
    }

    /// 装载账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            match self.entries.get(i) {
                Some(Some(e)) => {
                    h = (h ^ e.name_id as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ e.class as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ e.exit as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ e.missing as u64).wrapping_mul(FNV_PRIME);
                }
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        h = (h ^ self.n as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结（幂等），空账拒。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return match self.root {
                Some(r) => Ok(r),
                None => Ok(self.fold()),
            };
        }
        if self.n == 0 {
            return Err(self.note(SO_EEMPTY));
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
            None => return Err(self.note(SO_EMISMATCH)),
        };
        if self.fold() != root {
            return Err(self.note(SO_EMISMATCH));
        }
        Ok(root)
    }
}

/// 正常 64 位 x86-64 件模板（名号 + 尺寸 + 页同余）。
pub fn meta_ok(name_id: u16) -> ElfMeta {
    ElfMeta {
        name_id,
        size: 0x4000,
        magic: ELF_MAGIC,
        class: ELFCLASS64,
        machine: EM_X86_64,
        phnum: 4,
        vaddr: 0x2000,
        offset: 0x2000,
        needs: [0; MAX_NEEDS],
        need_n: 0,
    }
}

/// 基准解析表：名号 1/2/3 可解析。
pub fn resolver_base() -> Resolver {
    let mut r = Resolver::new();
    let _ = r.push(1);
    let _ = r.push(2);
    let _ = r.push(3);
    r
}

/// 六个坏件 + 一个正常件。
pub fn samples() -> ([ElfMeta; 7], Resolver) {
    let mut e0 = meta_ok(10);
    e0.size = 0;
    let mut e1 = meta_ok(11);
    e1.size = 40;
    let mut e2 = meta_ok(12);
    e2.class = ELFCLASS32;
    let mut e3 = meta_ok(13);
    e3.machine = EM_AARCH64;
    let mut e4 = meta_ok(14);
    e4.vaddr = 0x3000;
    e4.offset = 0x10;
    let mut e5 = meta_ok(15);
    e5.needs = [7, 0, 0, 0];
    e5.need_n = 1;
    let mut g = meta_ok(16);
    g.needs = [1, 2, 0, 0];
    g.need_n = 2;
    ([e0, e1, e2, e3, e4, e5, g], resolver_base())
}

/// 登入七个样本的装载账。
pub fn build_ledger() -> (SoLedger, bool) {
    let (metas, r) = samples();
    let mut l = SoLedger::new();
    let mut i = 0usize;
    while i < metas.len() {
        match metas.get(i) {
            Some(m) => {
                if l.record(m, &r).is_err() {
                    return (l, false);
                }
            }
            None => break,
        }
        i = i.wrapping_add(1);
    }
    let ok = l.freeze().is_ok();
    (l, ok)
}

/// 四类注入：名号非法 / 表序非升 / 同名重载 / 冻结与满席。
pub fn inject_four() -> [bool; 4] {
    // ① 名号为零与越界
    let mut a0 = SoLedger::new();
    let r = resolver_base();
    let m = meta_ok(0);
    let r0 = a0.record(&m, &r).err() == Some(SO_ENAME)
        && a0.record(&meta_ok(0x1000), &r).err() == Some(SO_EBADID);

    // ② 解析表非升序
    let mut a1 = SoLedger::new();
    let mut bad = Resolver::new();
    let _ = bad.push(3);
    let _ = bad.push(2);
    let r1 = a1.record(&meta_ok(5), &bad).err() == Some(SO_ERESOLVER);

    // ③ 同名已成功装载再登
    let mut a2 = SoLedger::new();
    let first = a2.record(&meta_ok(6), &r).is_ok();
    let r2 = first && a2.record(&meta_ok(6), &r).err() == Some(SO_EDUP)
        && a2.len() == 1
        && a2.loaded == 1;

    // ④ 冻结后登账 / 空账封 / 席位满
    let mut a3 = SoLedger::new();
    let _ = a3.record(&meta_ok(7), &r);
    let frozen = a3.freeze().is_ok();
    let blocked = a3.record(&meta_ok(8), &r).err() == Some(SO_EFROZEN);
    let mut empty = SoLedger::new();
    let e0 = empty.freeze().err() == Some(SO_EEMPTY);
    let mut full = SoLedger::new();
    let mut i = 0u16;
    while i < MAX_OBJS as u16 {
        let _ = full.record(&meta_ok(100u16.wrapping_add(i)), &r);
        i = i.wrapping_add(1);
    }
    let cap = full.record(&meta_ok(999), &r).err() == Some(SO_ECAP);
    let r3 = frozen && blocked && e0 && cap && full.len() == MAX_OBJS;

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11288 坏 so 装载错误文本族。
pub fn run_unx11288_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11288.badso_error_texts");
    let (metas, r) = samples();
    let g = |i: usize| -> ElfMeta {
        match metas.get(i) {
            Some(m) => *m,
            None => meta_ok(0),
        }
    };
    let v0 = diagnose(&g(0), &r);
    let v1 = diagnose(&g(1), &r);
    let v2 = diagnose(&g(2), &r);
    let v3 = diagnose(&g(3), &r);
    let v4 = diagnose(&g(4), &r);
    let v5 = diagnose(&g(5), &r);
    let v6 = diagnose(&g(6), &r);

    // SO1 空文件
    s.add(
        "SO1-空文件文本",
        v0.class == V_EMPTY && v0.text == TEXT_EMPTY && v0.exit == EXIT_BAD,
        "size=0 判空文件并出 invalid ELF header: empty file",
    );

    // SO2 截断
    s.add(
        "SO2-截断文本",
        v1.class == V_TRUNC && v1.text == TEXT_TRUNC,
        "size<64 判 file too short",
    );

    // SO3 错类
    s.add(
        "SO3-错类文本",
        v2.class == V_CLASS && v2.text == TEXT_CLASS,
        "ELFCLASS32 判 wrong ELF class: ELFCLASS32",
    );

    // SO4 错机
    s.add(
        "SO4-错机文本",
        v3.class == V_MACH && v3.text == TEXT_MACH,
        "EM_AARCH64 判 wrong ELF machine: EM_AARCH64",
    );

    // SO5 坏段
    s.add(
        "SO5-坏段文本",
        v4.class == V_SEG && v4.text == TEXT_SEG,
        "p_vaddr 与 p_offset 不同余于页判坏段",
    );

    // SO6 坏依赖（整串逐字）
    let dep_line = render(&v5, "libbad.so.1");
    s.add_dyn(
        "SO6-坏依赖整串",
        v5.class == V_DEP && v5.missing == 7
            && dep_line == "libbad.so.1: cannot open shared object file: No such file or directory",
        &format!("line={}", dep_line),
    );

    // SO7 正常件
    s.add(
        "SO7-正常装载",
        v6.class == V_OK && v6.exit == EXIT_OK && v6.text == TEXT_OK && v6.missing == 0,
        "六项判定全过则 loaded/exit 0",
    );

    // SO8 判定优先序
    let mut x0 = g(0);
    x0.class = ELFCLASS32;
    let mut x1 = g(1);
    x1.magic = 0;
    let mut x2 = meta_ok(20);
    x2.magic = 0;
    let mut x3 = meta_ok(21);
    x3.class = ELFCLASS32;
    x3.machine = EM_AARCH64;
    s.add(
        "SO8-判定优先序",
        diagnose(&x0, &r).class == V_EMPTY
            && diagnose(&x1, &r).class == V_TRUNC
            && diagnose(&x2, &r).class == V_EMPTY
            && diagnose(&x3, &r).class == V_CLASS,
        "尺寸先于魔数、魔数先于类别、类别先于机器",
    );

    // SO9 六类直方图
    let (l, ok) = build_ledger();
    let mut hist = true;
    let mut c = 1u8;
    while c <= 6 {
        if l.class_count(c) != 1 {
            hist = false;
        }
        c = c.wrapping_add(1);
    }
    s.add_dyn(
        "SO9-六类直方图",
        ok && hist && l.class_count(V_OK) == 1 && l.len() == 7,
        &format!("n={} ok={} c1={} c6={}", l.len(), l.class_count(V_OK),
                 l.class_count(1), l.class_count(6)),
    );

    // SO10 退出码统一
    let all127 = v0.exit == 127 && v1.exit == 127 && v2.exit == 127
        && v3.exit == 127 && v4.exit == 127 && v5.exit == 127;
    s.add_dyn(
        "SO10-退出码127",
        all127 && v6.exit == 0 && l.loaded == 1 && l.failed == 6,
        &format!("loaded={} failed={}", l.loaded, l.failed),
    );

    // SO11 名号与重载拒止
    let mut d = SoLedger::new();
    let n0 = d.record(&meta_ok(0), &r).err() == Some(SO_ENAME);
    let n1 = d.record(&meta_ok(0x1000), &r).err() == Some(SO_EBADID);
    let first = d.record(&meta_ok(30), &r).is_ok();
    let dup = d.record(&meta_ok(30), &r).err() == Some(SO_EDUP);
    s.add_dyn(
        "SO11-名号与重载拒止",
        n0 && n1 && first && dup && d.len() == 1 && d.rejects == 3,
        &format!("n={} rej={}", d.len(), d.rejects),
    );

    // SO12 冻结幂等 + 重放
    let mut f = l.clone();
    let a = f.freeze().unwrap_or(0);
    let b = f.freeze().unwrap_or(0);
    let rep = f.replay().unwrap_or(0);
    s.add_dyn(
        "SO12-冻结幂等与重放",
        a == b && a != 0 && rep == a && f.root_value() == Some(a),
        &format!("a={:#x} rep={:#x}", a, rep),
    );

    // SO13 注入四拒
    let hits = inject_four();
    s.add(
        "SO13-注入四拒",
        hits[0] && hits[1] && hits[2] && hits[3],
        "名号/表序/重载/冻结满席四路皆拒",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11288_badso_flow() {
        let (l, ok) = build_ledger();
        assert!(ok);
        assert_eq!(l.len(), 7);
        assert_eq!(l.loaded, 1);
        assert_eq!(l.failed, 6);
        let root = l.root_value();
        assert!(root.is_some());
        let mut f = l.clone();
        assert_eq!(f.replay().ok(), root);
        assert_eq!(f.freeze().ok(), root);
    }

    #[test]
    fn f11288_badso_gates() {
        let r = resolver_base();
        let mut l = SoLedger::new();
        assert_eq!(l.record(&meta_ok(0), &r), Err(SO_ENAME));
        assert_eq!(l.record(&meta_ok(0x1000), &r), Err(SO_EBADID));
        let mut bad = Resolver::new();
        assert!(bad.push(3));
        assert!(bad.push(2));
        assert_eq!(l.record(&meta_ok(1), &bad), Err(SO_ERESOLVER));
        assert!(l.record(&meta_ok(2), &r).is_ok());
        assert_eq!(l.record(&meta_ok(2), &r), Err(SO_EDUP));
        assert_eq!(l.rejects, 4);
        assert_eq!(l.accepts, 1);
        assert!(l.freeze().is_ok());
        assert_eq!(l.record(&meta_ok(3), &r), Err(SO_EFROZEN));
        assert_eq!(l.rejects, 5);
    }

    #[test]
    fn f11288_badso_rules() {
        let (metas, r) = samples();
        let v5 = diagnose(metas.get(5).expect("dep"), &r);
        assert_eq!(v5.class, V_DEP);
        assert_eq!(v5.missing, 7);
        assert_eq!(v5.exit, 127);
        let line = render(&v5, "libbad.so.1");
        assert_eq!(
            line,
            "libbad.so.1: cannot open shared object file: No such file or directory"
        );
        // 坏依赖只判缺失的那个，前面的依赖照常解析
        let mut m = meta_ok(40);
        m.needs = [1, 2, 9, 0];
        m.need_n = 3;
        assert_eq!(diagnose(&m, &r).missing, 9);
        // 空解析表：任何非空依赖都缺
        let e = Resolver::new();
        let mut m2 = meta_ok(41);
        m2.needs = [1, 0, 0, 0];
        m2.need_n = 1;
        assert_eq!(diagnose(&m2, &e).class, V_DEP);
        // 无依赖的空表仍可装载
        assert_eq!(diagnose(&meta_ok(42), &e).class, V_OK);
    }

    #[test]
    fn f11288_badso_injection() {
        let hits = inject_four();
        assert!(hits[0] && hits[1] && hits[2] && hits[3]);
    }

    #[test]
    fn f11288_checks_green() {
        let cs = run_unx11288_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
    }
}
