//! UNX-F11242 ld.so 自举重定位消费面（0x1f2a0 段）
//!
//! 锚点：UNX-F11242-J1 ld.so 自身 GOT/PLT 自重定位判据绿，自举后符号解析
//! 功能等价于基准机。
//!
//! 纯功能模型：动态装载器在能服务任何人之前，必须先把自己搬正——这就是
//! 自举。ld.so 把自己的段映射进来后进入 SELF_RELOCATING，先消费全部
//! R_X86_64_RELATIVE（这类重定位不需要符号表，只用基址+加数），再做
//! GLOB_DAT / JUMP_SLOT（只能引用自身 dynsym 里已定义的符号），完成后
//! 进入 SELF_RELOCATED，最后 serve 进入 SERVING 对外解析。
//!
//! 规则面：写序单调——RELATIVE 未清完之前不许碰符号类重定位，违者 EORDER；
//! 目标地址必须 8 字节对齐且在自身映射长度内；同一 offset 只许写一次；
//! 符号类重定位的符号下标必须存在于自身 dynsym 且 defined=1，引用外部
//! 符号（未定义）一律 EEXT，自举期不允许「先欠着」。
//!
//! 拒止面：阶段错（未映射即写、服务期再写、未完成即收尾、未收尾即服务）、
//! 重复 offset、未知重定位类型、越界、错位、负加数、符号下标越界、外部
//! 符号、解析未定义名、冻结后改写。收尾要求「注册的重定位全部落地」，
//! 少一条都不许进服务期。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2a0;
/// 阶段机不允许该动作
pub const LB_EORDER: u32 = SEG_BASE + 0x01;
/// 同一 offset 重复重定位
pub const LB_EDUP: u32 = SEG_BASE + 0x02;
/// 未知重定位类型
pub const LB_ETYPE: u32 = SEG_BASE + 0x03;
/// 目标越出自身映射
pub const LB_ERANGE: u32 = SEG_BASE + 0x04;
/// 目标未按 8 字节对齐
pub const LB_EALIGN: u32 = SEG_BASE + 0x05;
/// 符号下标越出自身 dynsym
pub const LB_ESYM: u32 = SEG_BASE + 0x06;
/// 引用外部未定义符号
pub const LB_EEXT: u32 = SEG_BASE + 0x07;
/// 负加数
pub const LB_EADD: u32 = SEG_BASE + 0x08;
/// 解析未定义符号名
pub const LB_EUNRES: u32 = SEG_BASE + 0x09;
/// 冻结后改写
pub const LB_EFROZEN: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// R_X86_64_RELATIVE
pub const RELATIVE: u8 = 8;
/// R_X86_64_GLOB_DAT
pub const GLOB_DAT: u8 = 6;
/// R_X86_64_JUMP_SLOT
pub const JUMP_SLOT: u8 = 7;
/// 重定位目标对齐
pub const ALIGN: u32 = 8;
/// 自身映射长度（16 KiB）
pub const SELF_SIZE: u32 = 0x2000;
/// 阶段：未映射
pub const PHASE_UNMAPPED: u8 = 0;
/// 阶段：已自映射
pub const PHASE_MAPPED: u8 = 1;
/// 阶段：自重定位中
pub const PHASE_RELOCATING: u8 = 2;
/// 阶段：自重定位完成
pub const PHASE_RELOCATED: u8 = 3;
/// 阶段：服务中
pub const PHASE_SERVING: u8 = 4;
/// 阶段：冻结
pub const PHASE_FROZEN: u8 = 5;
/// 基准机符号值表（自举后解析必须逐项相等）
pub const BASELINE: [(&str, u64); 3] = [
    ("_dl_start", 0x1000),
    ("_dl_relocate", 0x1100),
    ("_rtld_global", 0x1800),
];
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- 数据结构
/// 一条重定位：目标偏移 / 类型 / 符号下标 / 加数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reloc {
    pub offset: u32,
    pub rtype: u8,
    pub sym: u16,
    pub addend: i64,
}

impl Reloc {
    /// 相对重定位（无需符号）。
    pub fn relative(offset: u32, addend: i64) -> Reloc {
        Reloc { offset, rtype: RELATIVE, sym: 0, addend }
    }

    /// GOT 数据重定位。
    pub fn glob(offset: u32, sym: u16) -> Reloc {
        Reloc { offset, rtype: GLOB_DAT, sym, addend: 0 }
    }

    /// PLT 跳转槽重定位。
    pub fn jump(offset: u32, sym: u16) -> Reloc {
        Reloc { offset, rtype: JUMP_SLOT, sym, addend: 0 }
    }
}

/// 自身 dynsym 的一项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sym {
    pub name: &'static str,
    pub value: u64,
    pub defined: bool,
}

impl Sym {
    /// 内部已定义符号。
    pub fn internal(name: &'static str, value: u64) -> Sym {
        Sym { name, value, defined: true }
    }

    /// 外部未定义符号。
    pub fn external(name: &'static str) -> Sym {
        Sym { name, value: 0, defined: false }
    }
}

/// ld.so 自举账：映射长度 + 重定位表 + 自身符号表 + 阶段机。
#[derive(Clone, Debug)]
pub struct SelfBootstrap {
    size: u32,
    relocs: Vec<Reloc>,
    applied: Vec<u32>,
    syms: Vec<Sym>,
    phase: u8,
    root: Option<u64>,
    /// 接受的重定位数
    pub accepts: u64,
    /// 拒止数
    pub rejects: u64,
    /// 已落地 RELATIVE 数
    pub relatives: u64,
    /// 已落地符号类重定位数
    pub symbol_relocs: u64,
    /// 解析成功次数
    pub resolves: u64,
}

impl SelfBootstrap {
    /// 空账。
    pub fn new() -> Self {
        SelfBootstrap {
            size: 0,
            relocs: Vec::new(),
            applied: Vec::new(),
            syms: Vec::new(),
            phase: PHASE_UNMAPPED,
            root: None,
            accepts: 0,
            rejects: 0,
            relatives: 0,
            symbol_relocs: 0,
            resolves: 0,
        }
    }

    /// 当前阶段。
    pub fn phase(&self) -> u8 {
        self.phase
    }

    /// 登记自身符号（映射前）。
    pub fn push_sym(&mut self, s: Sym) -> Result<(), u32> {
        if self.phase != PHASE_UNMAPPED {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(LB_EORDER);
        }
        self.syms.push(s);
        Ok(())
    }

    /// 登记一条待写重定位（映射前）。
    pub fn push_reloc(&mut self, r: Reloc) -> Result<(), u32> {
        if self.phase != PHASE_UNMAPPED {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(LB_EORDER);
        }
        let mut i = 0usize;
        while i < self.relocs.len() {
            if let Some(x) = self.relocs.get(i) {
                if x.offset == r.offset {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(LB_EDUP);
                }
            }
            i = i.wrapping_add(1);
        }
        self.relocs.push(r);
        Ok(())
    }

    /// 自映射：长度非零且 8 字节对齐。
    pub fn map_self(&mut self, size: u32) -> Result<u64, u32> {
        if self.phase == PHASE_FROZEN {
            return self.fail(LB_EFROZEN);
        }
        if self.phase != PHASE_UNMAPPED {
            return self.fail(LB_EORDER);
        }
        if size == 0 || size % ALIGN != 0 {
            return self.fail(LB_ERANGE);
        }
        self.size = size;
        self.phase = PHASE_MAPPED;
        Ok(self.fold())
    }

    /// 进入自重定位期。
    pub fn begin(&mut self) -> Result<u64, u32> {
        if self.phase == PHASE_FROZEN {
            return self.fail(LB_EFROZEN);
        }
        if self.phase != PHASE_MAPPED {
            return self.fail(LB_EORDER);
        }
        self.phase = PHASE_RELOCATING;
        Ok(self.fold())
    }

    /// 已注册但未落地的 RELATIVE 条数。
    pub fn relatives_pending(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.relocs.len() {
            if let Some(r) = self.relocs.get(i) {
                if r.rtype == RELATIVE && !self.is_applied(r.offset) {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 某 offset 是否已落地。
    pub fn is_applied(&self, offset: u32) -> bool {
        let mut i = 0usize;
        while i < self.applied.len() {
            if self.applied.get(i).copied() == Some(offset) {
                return true;
            }
            i = i.wrapping_add(1);
        }
        false
    }

    /// 已落地条数。
    pub fn applied_count(&self) -> usize {
        self.applied.len()
    }

    /// 注册条数。
    pub fn reloc_count(&self) -> usize {
        self.relocs.len()
    }

    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }
}

impl SelfBootstrap {
    /// 写一条重定位。优先级：阶段 → 重复 → 对齐 → 越界 → 类型/符号。
    pub fn apply(&mut self, r: Reloc) -> Result<u64, u32> {
        if self.phase == PHASE_FROZEN {
            return self.fail(LB_EFROZEN);
        }
        if self.phase != PHASE_RELOCATING {
            return self.fail(LB_EORDER);
        }
        if self.is_applied(r.offset) {
            return self.fail(LB_EDUP);
        }
        if r.offset % ALIGN != 0 {
            return self.fail(LB_EALIGN);
        }
        if r.offset.saturating_add(ALIGN) > self.size {
            return self.fail(LB_ERANGE);
        }
        match r.rtype {
            RELATIVE => {
                if r.addend < 0 {
                    return self.fail(LB_EADD);
                }
                self.relatives = self.relatives.wrapping_add(1);
            }
            GLOB_DAT | JUMP_SLOT => {
                if self.relatives_pending() != 0 {
                    return self.fail(LB_EORDER);
                }
                let s = match self.syms.get(r.sym as usize) {
                    Some(s) => *s,
                    None => return self.fail(LB_ESYM),
                };
                if !s.defined {
                    return self.fail(LB_EEXT);
                }
                self.symbol_relocs = self.symbol_relocs.wrapping_add(1);
            }
            _ => return self.fail(LB_ETYPE),
        }
        self.applied.push(r.offset);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 收尾：注册的重定位必须全部落地，才可进 SELF_RELOCATED。
    pub fn finish(&mut self) -> Result<u64, u32> {
        if self.phase == PHASE_FROZEN {
            return self.fail(LB_EFROZEN);
        }
        if self.phase != PHASE_RELOCATING {
            return self.fail(LB_EORDER);
        }
        if self.relocs.is_empty() || self.applied.len() != self.relocs.len() {
            return self.fail(LB_EORDER);
        }
        if self.relatives_pending() != 0 {
            return self.fail(LB_EORDER);
        }
        self.phase = PHASE_RELOCATED;
        Ok(self.fold())
    }

    /// 进入服务期：只有收尾完成的账才能对外解析。
    pub fn serve(&mut self) -> Result<u64, u32> {
        if self.phase == PHASE_FROZEN {
            return self.fail(LB_EFROZEN);
        }
        if self.phase != PHASE_RELOCATED {
            return self.fail(LB_EORDER);
        }
        self.phase = PHASE_SERVING;
        Ok(self.fold())
    }

    /// 按名解析自身符号；未定义或不存在一律 EUNRES。
    pub fn resolve(&mut self, name: &str) -> Result<u64, u32> {
        if self.phase == PHASE_FROZEN {
            return self.fail(LB_EFROZEN);
        }
        if self.phase != PHASE_SERVING {
            return self.fail(LB_EORDER);
        }
        let mut i = 0usize;
        while i < self.syms.len() {
            if let Some(s) = self.syms.get(i) {
                if s.name == name {
                    if !s.defined {
                        return self.fail(LB_EUNRES);
                    }
                    self.resolves = self.resolves.wrapping_add(1);
                    return Ok(s.value);
                }
            }
            i = i.wrapping_add(1);
        }
        self.fail(LB_EUNRES)
    }

    /// 已落地 offset 是否都能对上注册表且无重复（回放自检）。
    pub fn violation_scan(&self) -> usize {
        let mut bad = 0usize;
        let mut i = 0usize;
        while i < self.applied.len() {
            let off = self.applied.get(i).copied().unwrap_or(u32::MAX);
            let mut hits = 0usize;
            let mut k = 0usize;
            while k < self.relocs.len() {
                if let Some(r) = self.relocs.get(k) {
                    if r.offset == off {
                        hits = hits.wrapping_add(1);
                    }
                }
                k = k.wrapping_add(1);
            }
            let mut dup = 0usize;
            let mut j = 0usize;
            while j < self.applied.len() {
                if self.applied.get(j).copied() == Some(off) {
                    dup = dup.wrapping_add(1);
                }
                j = j.wrapping_add(1);
            }
            if hits != 1 || dup != 1 {
                bad = bad.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        bad
    }

    /// FNV-1a：映射长度 + 重定位表 + 符号表 + 落地表 + 计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ (self.size as u64)).wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < self.relocs.len() {
            if let Some(r) = self.relocs.get(i) {
                h = (h ^ (r.offset as u64)).wrapping_mul(FNV_PRIME);
                h = (h ^ (r.rtype as u64)).wrapping_mul(FNV_PRIME);
                h = (h ^ (r.sym as u64)).wrapping_mul(FNV_PRIME);
                h = (h ^ (r.addend as u64)).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        let mut k = 0usize;
        while k < self.syms.len() {
            if let Some(s) = self.syms.get(k) {
                h = fnv_bytes(h, s.name.as_bytes());
                h = (h ^ s.value).wrapping_mul(FNV_PRIME);
                h = (h ^ (s.defined as u64)).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < self.applied.len() {
            if let Some(o) = self.applied.get(j) {
                h = (h ^ (*o as u64)).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        h = (h ^ self.accepts).wrapping_mul(FNV_PRIME);
        h = (h ^ self.symbol_relocs).wrapping_mul(FNV_PRIME);
        h = (h ^ self.resolves).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结：只允许在服务期冻结，幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.phase == PHASE_FROZEN {
            return match self.root {
                Some(v) => Ok(v),
                None => self.fail(LB_EORDER),
            };
        }
        if self.phase != PHASE_SERVING {
            return self.fail(LB_EORDER);
        }
        self.phase = PHASE_FROZEN;
        let v = self.fold();
        self.root = Some(v);
        Ok(v)
    }

    /// 回放：结构自检 + 指纹比对。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.phase != PHASE_FROZEN {
            return self.fail(LB_EORDER);
        }
        if self.violation_scan() != 0 {
            return self.fail(LB_EDUP);
        }
        let v = self.fold();
        if Some(v) != self.root {
            return self.fail(LB_EORDER);
        }
        Ok(v)
    }

    /// 冻结值。
    pub fn root(&self) -> Option<u64> {
        self.root
    }
}

fn fnv_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = (h ^ (*b as u64)).wrapping_mul(FNV_PRIME);
        }
        i = i.wrapping_add(1);
    }
    h
}

/// 自身符号表（4 项，含 1 项外部未定义）。
pub const SELF_SYMS: [(&str, u64, bool); 4] = [
    ("_dl_start", 0x1000, true),
    ("_dl_relocate", 0x1100, true),
    ("memcpy", 0, false),
    ("_rtld_global", 0x1800, true),
];

/// 自重定位表：四条 RELATIVE 在前，两条 GLOB_DAT、两条 JUMP_SLOT 在后。
pub fn self_relocs() -> [Reloc; 8] {
    [
        Reloc::relative(0x200, 0x1000),
        Reloc::relative(0x208, 0x1200),
        Reloc::relative(0x210, 0x1400),
        Reloc::relative(0x230, 0x1600),
        Reloc::glob(0x218, 0),
        Reloc::glob(0x220, 3),
        Reloc::jump(0x228, 1),
        Reloc::jump(0x238, 1),
    ]
}

/// 状态：已映射并进入自重定位期（尚未写任何条目）。
pub fn relocating() -> (SelfBootstrap, bool) {
    let mut b = SelfBootstrap::new();
    let mut i = 0usize;
    while i < SELF_SYMS.len() {
        if let Some(row) = SELF_SYMS.get(i) {
            if b.push_sym(Sym { name: row.0, value: row.1, defined: row.2 }).is_err() {
                return (b, false);
            }
        }
        i = i.wrapping_add(1);
    }
    let relocs = self_relocs();
    let mut k = 0usize;
    while k < relocs.len() {
        if let Some(r) = relocs.get(k) {
            if b.push_reloc(*r).is_err() {
                return (b, false);
            }
        }
        k = k.wrapping_add(1);
    }
    let ok = b.map_self(SELF_SIZE).is_ok() && b.begin().is_ok();
    (b, ok)
}

/// 建场（服务期）：自映射 → 全部重定位 → 收尾 → 服务 → 基准解析。
pub fn build_serving() -> (SelfBootstrap, bool) {
    let (mut b, ok0) = relocating();
    if !ok0 {
        return (b, false);
    }
    let relocs = self_relocs();
    let mut k = 0usize;
    let mut ok = true;
    while k < relocs.len() {
        if let Some(r) = relocs.get(k) {
            if b.apply(*r).is_err() {
                ok = false;
            }
        }
        k = k.wrapping_add(1);
    }
    if !ok || b.finish().is_err() || b.serve().is_err() {
        return (b, false);
    }
    let mut i = 0usize;
    while i < BASELINE.len() {
        if let Some(row) = BASELINE.get(i) {
            if b.resolve(row.0).ok() != Some(row.1) {
                return (b, false);
            }
        }
        i = i.wrapping_add(1);
    }
    (b, true)
}

/// 建场（终态）：服务期账再冻结并回放。
pub fn build_bootstrap() -> (SelfBootstrap, bool) {
    let (mut b, ok0) = build_serving();
    if !ok0 {
        return (b, false);
    }
    let ok = b.freeze().is_ok() && b.replay().is_ok();
    (b, ok)
}

/// 四类独立注入：错位 / 越界 / 外部符号 / 未映射即写。
pub fn inject_four() -> [bool; 4] {
    // ① 错位目标
    let (mut a0, s0) = relocating();
    let r0 = s0
        && a0.apply(Reloc::relative(0x204, 0x10)).err() == Some(LB_EALIGN)
        && a0.applied_count() == 0;

    // ② 越界目标
    let (mut a1, s1) = relocating();
    let r1 = s1
        && a1.apply(Reloc::relative(SELF_SIZE, 0x10)).err() == Some(LB_ERANGE)
        && a1.apply(Reloc::relative(SELF_SIZE - ALIGN, 0x10)).is_ok();

    // ③ 外部未定义符号（relatives 已清）
    let (mut a2, s2) = relocating();
    let relocs = self_relocs();
    let mut i = 0usize;
    let mut ok2 = true;
    while i < 4 {
        if let Some(r) = relocs.get(i) {
            if a2.apply(*r).is_err() {
                ok2 = false;
            }
        }
        i = i.wrapping_add(1);
    }
    let r2 = s2
        && ok2
        && a2.relatives_pending() == 0
        && a2.apply(Reloc::glob(0x218, 2)).err() == Some(LB_EEXT)
        && a2.apply(Reloc::glob(0x218, 9)).err() == Some(LB_ESYM);

    // ④ 未映射即写、未映射即收尾
    let mut a3 = SelfBootstrap::new();
    let r3 = a3.apply(Reloc::relative(0x200, 0)).err() == Some(LB_EORDER)
        && a3.finish().err() == Some(LB_EORDER)
        && a3.serve().err() == Some(LB_EORDER);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11242 ld.so 自举重定位消费面。
pub fn run_unx11242_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11242");
    let (mut b, ok) = build_bootstrap();

    // LB1 阶段机全序
    let (mut p1, s1) = relocating();
    let phase_ok = s1
        && p1.phase() == PHASE_RELOCATING
        && p1.reloc_count() == 8
        && p1.applied_count() == 0;
    s.add_dyn(
        "LB1-阶段机全序",
        ok && b.phase() == PHASE_FROZEN && phase_ok,
        &format!("phase={} reg={} applied={}", b.phase(), p1.reloc_count(), p1.applied_count()),
    );

    // LB2 RELATIVE 先行
    let (mut p2, s2) = relocating();
    let early = p2.apply(Reloc::glob(0x218, 0)).err() == Some(LB_EORDER);
    let mut ok2 = s2 && early && p2.relatives_pending() == 4;
    let relocs = self_relocs();
    let mut i = 0usize;
    while i < 4 {
        if let Some(r) = relocs.get(i) {
            if p2.apply(*r).is_err() {
                ok2 = false;
            }
        }
        i = i.wrapping_add(1);
    }
    s.add(
        "LB2-RELATIVE 先行",
        ok2 && p2.relatives_pending() == 0 && p2.apply(Reloc::glob(0x218, 0)).is_ok(),
        "符号类重定位在 RELATIVE 清完前一律 EORDER",
    );

    // LB3 符号自给自足
    let (mut p3, s3) = relocating();
    let mut cleared = true;
    let mut k = 0usize;
    while k < 4 {
        if let Some(r) = relocs.get(k) {
            if p3.apply(*r).is_err() {
                cleared = false;
            }
        }
        k = k.wrapping_add(1);
    }
    s.add(
        "LB3-符号自给自足",
        s3 && cleared
            && p3.apply(Reloc::glob(0x218, 2)).err() == Some(LB_EEXT)
            && p3.apply(Reloc::jump(0x220, 7)).err() == Some(LB_ESYM),
        "自举期只许引用自身已定义符号",
    );

    // LB4 对齐拒止
    let (mut p4, s4) = relocating();
    s.add(
        "LB4-对齐拒止",
        s4 && p4.apply(Reloc::relative(0x201, 0)).err() == Some(LB_EALIGN)
            && p4.apply(Reloc::relative(0x208, 0)).is_ok(),
        "目标未按 8 字节对齐即拒",
    );

    // LB5 越界拒止
    let (mut p5, s5) = relocating();
    s.add_dyn(
        "LB5-越界拒止",
        s5 && p5.apply(Reloc::relative(SELF_SIZE, 0)).err() == Some(LB_ERANGE)
            && p5.apply(Reloc::relative(SELF_SIZE - ALIGN, 0)).is_ok(),
        &format!("limit={:#x}", SELF_SIZE),
    );

    // LB6 重复拒止
    let (mut p6, s6) = relocating();
    let first = p6.apply(Reloc::relative(0x2f8, 0x40)).is_ok();
    s.add(
        "LB6-重复拒止",
        s6 && first && p6.apply(Reloc::relative(0x2f8, 0x80)).err() == Some(LB_EDUP)
            && p6.applied_count() == 1,
        "同 offset 只许写一次",
    );

    // LB7 未知类型拒止
    let (mut p7, s7) = relocating();
    let odd = Reloc { offset: 0x300, rtype: 20, sym: 0, addend: 0 };
    s.add(
        "LB7-未知类型拒止",
        s7 && p7.apply(odd).err() == Some(LB_ETYPE) && p7.accepts == 0,
        "自举期只认 RELATIVE/GLOB_DAT/JUMP_SLOT",
    );

    // LB8 负加数拒止
    let (mut p8, s8) = relocating();
    s.add(
        "LB8-负加数拒止",
        s8 && p8.apply(Reloc::relative(0x308, -8)).err() == Some(LB_EADD)
            && p8.apply(Reloc::relative(0x308, 8)).is_ok(),
        "RELATIVE 加数为负即在自身镜像外，拒",
    );

    // LB9 未完成即收尾 / 未收尾即服务
    let (mut p9, s9) = relocating();
    let f9 = p9.finish().err() == Some(LB_EORDER);
    let s9b = p9.serve().err() == Some(LB_EORDER);
    s.add(
        "LB9-未完成即收尾拒",
        s9 && f9 && s9b && p9.phase() == PHASE_RELOCATING,
        "注册八条一条未落地，收尾与服务双拒",
    );

    // LB10 解析等价基准机
    let (mut p10, s10) = build_serving();
    let mut same = true;
    let mut i = 0usize;
    while i < BASELINE.len() {
        if let Some(row) = BASELINE.get(i) {
            if p10.resolve(row.0).ok() != Some(row.1) {
                same = false;
            }
        }
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "LB10-解析等价基准机",
        s10 && same && p10.resolves == 6 && p10.phase() == PHASE_SERVING,
        &format!("resolves={} same={}", p10.resolves, same as u8),
    );

    // LB11 冻结幂等与回放
    let f1 = b.freeze();
    let f2 = b.freeze();
    let rp = b.replay();
    s.add_dyn(
        "LB11-冻结幂等回放",
        ok && f1 == f2 && f1.is_ok() && f1.ok() == b.root() && rp.ok() == b.root(),
        &format!("root={:x?} scan={}", b.root(), b.violation_scan()),
    );

    // LB12 未定义名拒止
    let (mut p12, s12) = build_serving();
    let ext = p12.resolve("memcpy").err() == Some(LB_EUNRES);
    let gone = p12.resolve("_dl_no_such").err() == Some(LB_EUNRES);
    s.add(
        "LB12-未定义名拒止",
        s12 && ext && gone && p12.resolves == 3,
        "外部未定义名与不存在名同拒 EUNRES",
    );

    // LB13 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < hits.len() {
        if !hits.get(k).copied().unwrap_or(false) {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    s.add_dyn(
        "LB13-注入四拒",
        all && b.rejects == 0 && b.applied_count() == 8 && b.symbol_relocs == 4,
        &format!(
            "inject={}{}{}{} applied={} sym={}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            b.applied_count(), b.symbol_relocs
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11242_bootstrap_flow() {
        let (mut b, ok) = build_bootstrap();
        assert!(ok);
        assert_eq!(b.phase(), PHASE_FROZEN);
        assert_eq!(b.reloc_count(), 8);
        assert_eq!(b.applied_count(), 8);
        assert_eq!(b.relatives, 4);
        assert_eq!(b.symbol_relocs, 4);
        assert_eq!(b.resolves, 3);
        assert_eq!(b.rejects, 0);
        assert_eq!(b.violation_scan(), 0);
        let root = b.root();
        assert_eq!(root, Some(b.fold()));
        assert_eq!(b.freeze().ok(), root);
        assert_eq!(b.replay().ok(), root);
    }

    #[test]
    fn f11242_gates_reject_count() {
        let mut b = SelfBootstrap::new();
        assert_eq!(b.apply(Reloc::relative(0, 0)), Err(LB_EORDER));
        assert_eq!(b.begin(), Err(LB_EORDER));
        assert_eq!(b.map_self(0), Err(LB_ERANGE));
        assert_eq!(b.map_self(0x1004), Err(LB_ERANGE));
        assert!(b.map_self(SELF_SIZE).is_ok());
        assert_eq!(b.push_sym(Sym::internal("x", 1)), Err(LB_EORDER));
        assert_eq!(b.push_reloc(Reloc::relative(0x100, 0)), Err(LB_EORDER));
        assert!(b.begin().is_ok());
        assert_eq!(b.apply(Reloc::relative(0x104, 0)), Err(LB_EALIGN));
        assert_eq!(b.apply(Reloc::relative(SELF_SIZE, 0)), Err(LB_ERANGE));
        assert_eq!(b.apply(Reloc { offset: 0x300, rtype: 20, sym: 0, addend: 0 }), Err(LB_ETYPE));
        assert_eq!(b.apply(Reloc::relative(0x108, -1)), Err(LB_EADD));
        assert_eq!(b.apply(Reloc::glob(0x110, 0)), Err(LB_ESYM));
        assert!(b.apply(Reloc::relative(0x118, 0x10)).is_ok());
        assert_eq!(b.apply(Reloc::relative(0x118, 0x20)), Err(LB_EDUP));
        assert_eq!(b.finish(), Err(LB_EORDER));
        assert_eq!(b.serve(), Err(LB_EORDER));
        assert_eq!(b.resolve("_dl_start"), Err(LB_EORDER));
        assert_eq!(b.rejects, 15);
        assert_eq!(b.accepts, 1);
        assert_eq!(b.applied_count(), 1);
    }

    #[test]
    fn f11242_rules_ordering() {
        let (mut b, ok) = relocating();
        assert!(ok);
        assert_eq!(b.relatives_pending(), 4);
        assert_eq!(b.apply(Reloc::glob(0x218, 0)), Err(LB_EORDER));
        assert_eq!(b.finish(), Err(LB_EORDER));
        assert_eq!(b.serve(), Err(LB_EORDER));
        let relocs = self_relocs();
        let mut i = 0usize;
        while i < 4 {
            assert!(b.apply(relocs[i]).is_ok());
            i = i.wrapping_add(1);
        }
        assert_eq!(b.relatives_pending(), 0);
        assert_eq!(b.finish(), Err(LB_EORDER));
        assert!(b.apply(Reloc::glob(0x218, 0)).is_ok());
        assert!(b.apply(Reloc::glob(0x220, 3)).is_ok());
        assert!(b.apply(Reloc::jump(0x228, 1)).is_ok());
        assert!(b.apply(Reloc::jump(0x238, 1)).is_ok());
        assert!(b.finish().is_ok());
        assert!(b.serve().is_ok());
        assert_eq!(b.resolve("_rtld_global"), Ok(0x1800));
        assert_eq!(b.resolve("memcpy"), Err(LB_EUNRES));
        assert!(b.freeze().is_ok());
        assert_eq!(b.resolve("_dl_start"), Err(LB_EFROZEN));
    }

    #[test]
    fn f11242_injection_four() {
        let hits = inject_four();
        assert!(hits.iter().all(|x| *x));
    }

    #[test]
    fn f11242_checks_green() {
        let cs = run_unx11242_checks();
        assert!(cs.all_passed());
        assert_eq!(cs.len(), 13);
    }
}
