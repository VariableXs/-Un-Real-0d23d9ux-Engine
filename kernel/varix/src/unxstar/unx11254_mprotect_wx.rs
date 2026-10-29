//! UNX-F11254 mprotect 段保护切换消费（0x1f360 段）
//!
//! 锚点：UNX-F11254-J1 R-X/R--/RW- 切换消费判据绿，W^X 语义与 Linux 一致，
//! 违权限访问触发 SEGV。
//!
//! 纯功能模型：装载完成后内核要多次改段权限：文本段 R-X 常驻；只读数据 R--；
//! 数据段先 RW- 供重定位写入，RELRO 收尾后降为 R--；栈是 RW- 且永不置 X
//! （NX）。本模块把每次 `mprotect` 写成显式状态迁移，并守住一条预算：
//! **任何时刻都不允许同一区间同时带 W 和 X**（W^X）。试图一步跳到 RWX 的
//! 请求在策略层就被拒，根本不会到达页表。
//!
//! 规则面：区间页对齐、非空、不重叠、权限非零且不超三位；切换必须经 R 中转
//! （不能从 PROT_NONE 直接开 W）；RELRO 只接受「RW → R」这一种形态；访问按
//! 当前权限逐位判定，缺位即 SEGV 并记账。
//!
//! 判据核心：一次完整生命周期 R-X → R → RW → R → R-X 全程无 W|X，且每次
//! 降权后越权访问都稳定触发 SEGV。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f360;
/// 权限值非法（零位 / 超三位 / 访问种类非法）
pub const MP_EPROT: u32 = SEG_BASE + 0x01;
/// 请求同时带 W 与 X
pub const MP_EWX: u32 = SEG_BASE + 0x02;
/// 区间未对齐、越界或重叠
pub const MP_EBOUNDS: u32 = SEG_BASE + 0x03;
/// 阶段错（未映射即切换 / 切换未过 R 中转）
pub const MP_EORDER: u32 = SEG_BASE + 0x04;
/// 区间下标越界或重放摘要不符
pub const MP_ESTATE: u32 = SEG_BASE + 0x05;
/// 冻结后改写
pub const MP_EFROZEN: u32 = SEG_BASE + 0x06;
/// 违权限访问（SEGV）
pub const MP_ESEGV: u32 = SEG_BASE + 0x07;
/// RELRO 前置不满足（非 RW 或目标非 R）
pub const MP_ERELRO: u32 = SEG_BASE + 0x08;
/// 区间起点重号
pub const MP_EDUP: u32 = SEG_BASE + 0x09;
/// 区间表满
pub const MP_EFULL: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// 页大小
pub const PAGE: u64 = 0x1000;
/// 区间表容量
pub const REGION_CAP: usize = 6;
/// 权限：无
pub const PROT_NONE: u8 = 0x0;
/// 权限：可读（ELF PF_R）
pub const PROT_R: u8 = 0x4;
/// 权限：可写（ELF PF_W）
pub const PROT_W: u8 = 0x2;
/// 权限：可执行（ELF PF_X）
pub const PROT_X: u8 = 0x1;
/// 访问种类：读
pub const ACC_R: u8 = 0x4;
/// 访问种类：写
pub const ACC_W: u8 = 0x2;
/// 访问种类：执行
pub const ACC_X: u8 = 0x1;
/// 状态：建场
pub const ST_BUILD: u8 = 0;
/// 状态：已映射
pub const ST_MAPPED: u8 = 1;
/// 状态：冻结
pub const ST_FROZEN: u8 = 2;
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- 区间
/// 一枚已映射区间：页对齐起点、页数、当前权限与切换计数。
#[derive(Copy, Clone)]
pub struct ProtRegion {
    vaddr: u64,
    pages: u8,
    prot: u8,
    relro_done: bool,
    switches: u8,
    accesses: u16,
    faults: u16,
}

impl ProtRegion {
    /// 起始虚址。
    pub fn vaddr(&self) -> u64 {
        self.vaddr
    }

    /// 结束虚址（不含）。
    pub fn end(&self) -> u64 {
        self.vaddr.wrapping_add(self.pages as u64 * PAGE)
    }

    /// 页数。
    pub fn pages(&self) -> u8 {
        self.pages
    }

    /// 当前权限位。
    pub fn prot(&self) -> u8 {
        self.prot
    }

    /// RELRO 是否已收尾。
    pub fn relro_done(&self) -> bool {
        self.relro_done
    }

    /// 成功切换次数。
    pub fn switches(&self) -> u8 {
        self.switches
    }

    /// 访问次数。
    pub fn accesses(&self) -> u16 {
        self.accesses
    }

    /// 违权访问次数。
    pub fn faults(&self) -> u16 {
        self.faults
    }

    /// 是否同时带 W 与 X（W^X 违例）。
    pub fn wx(&self) -> bool {
        self.prot & PROT_W != 0 && self.prot & PROT_X != 0
    }
}

// ---------------------------------------------------------------- 映射表
/// 段保护表：建场映射 → 封场 → 逐次切换 → 冻结重放。
pub struct ProtMap {
    regions: [Option<ProtRegion>; REGION_CAP],
    count: usize,
    state: u8,
    root: u64,
    guards: u32,
    denies: u32,
    segv: u32,
    relro: u32,
    rejects: u32,
}

impl ProtMap {
    /// 空表，状态为建场。
    pub fn new() -> Self {
        Self {
            regions: [None; REGION_CAP],
            count: 0,
            state: ST_BUILD,
            root: 0,
            guards: 0,
            denies: 0,
            segv: 0,
            relro: 0,
            rejects: 0,
        }
    }

    /// 记一次拒绝并返回错误码。
    fn reject<T>(&mut self, code: u32) -> Result<T, u32> {
        self.denies = self.denies.wrapping_add(1);
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 按值取出区间副本（避免索引与借用冲突）。
    fn peek(&self, idx: usize) -> Option<ProtRegion> {
        if idx >= self.count {
            return None;
        }
        match self.regions.get(idx) {
            Some(Some(r)) => Some(*r),
            _ => None,
        }
    }

    /// 建场期映射一枚区间：页对齐、非空、不重叠、W^X 合规。
    pub fn map(&mut self, vaddr: u64, size: u64, prot: u8) -> Result<usize, u32> {
        if self.state == ST_FROZEN {
            return self.reject(MP_EFROZEN);
        }
        if self.state != ST_BUILD {
            return self.reject(MP_EORDER);
        }
        if prot == PROT_NONE || prot & !(PROT_R | PROT_W | PROT_X) != 0 {
            return self.reject(MP_EPROT);
        }
        if prot & PROT_W != 0 && prot & PROT_X != 0 {
            return self.reject(MP_EWX);
        }
        if vaddr % PAGE != 0 || size == 0 || size % PAGE != 0 || size / PAGE > 255 {
            return self.reject(MP_EBOUNDS);
        }
        let end = match vaddr.checked_add(size) {
            Some(e) => e,
            None => return self.reject(MP_EBOUNDS),
        };
        if self.count >= REGION_CAP {
            return self.reject(MP_EFULL);
        }
        let mut i = 0;
        while i < REGION_CAP {
            match self.regions.get(i) {
                Some(Some(r)) => {
                    if r.vaddr == vaddr {
                        return self.reject(MP_EDUP);
                    }
                    if vaddr < r.end() && r.vaddr < end {
                        return self.reject(MP_EBOUNDS);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let region = ProtRegion {
            vaddr,
            pages: (size / PAGE) as u8,
            prot,
            relro_done: false,
            switches: 0,
            accesses: 0,
            faults: 0,
        };
        let idx = self.count;
        match self.regions.get_mut(idx) {
            Some(slot) => {
                *slot = Some(region);
            }
            None => return self.reject(MP_ESTATE),
        }
        self.count = idx + 1;
        self.guards = self.guards.wrapping_add(1);
        Ok(idx)
    }

    /// 封场：建场结束后才能切换权限，且至少要有一枚区间。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state == ST_FROZEN {
            return self.reject(MP_EFROZEN);
        }
        if self.state != ST_BUILD {
            return self.reject(MP_EORDER);
        }
        if self.count == 0 {
            return self.reject(MP_ESTATE);
        }
        self.state = ST_MAPPED;
        self.guards = self.guards.wrapping_add(1);
        Ok(())
    }

    /// 切换权限：W^X 优先，其次 RELRO 锁定，再次 R 中转规则。
    pub fn protect(&mut self, idx: usize, new_prot: u8) -> Result<u64, u32> {
        if self.state == ST_FROZEN {
            return self.reject(MP_EFROZEN);
        }
        if self.state != ST_MAPPED {
            return self.reject(MP_EORDER);
        }
        if new_prot == PROT_NONE || new_prot & !(PROT_R | PROT_W | PROT_X) != 0 {
            return self.reject(MP_EPROT);
        }
        if new_prot & PROT_W != 0 && new_prot & PROT_X != 0 {
            return self.reject(MP_EWX);
        }
        let old = match self.peek(idx) {
            Some(r) => r,
            None => return self.reject(MP_ESTATE),
        };
        if old.relro_done && new_prot & PROT_W != 0 {
            return self.reject(MP_ERELRO);
        }
        if old.prot & PROT_R == 0 || new_prot & PROT_R == 0 {
            return self.reject(MP_EORDER);
        }
        match self.regions.get_mut(idx) {
            Some(Some(slot)) => {
                slot.prot = new_prot;
                slot.switches = slot.switches.wrapping_add(1);
            }
            _ => return self.reject(MP_ESTATE),
        }
        self.guards = self.guards.wrapping_add(1);
        Ok(self.fold())
    }

    /// 按当前权限做一次访问：缺位即 SEGV 并记账。
    pub fn access(&mut self, idx: usize, kind: u8) -> Result<u64, u32> {
        if self.state == ST_BUILD {
            return self.reject(MP_EORDER);
        }
        if kind != ACC_R && kind != ACC_W && kind != ACC_X {
            return self.reject(MP_EPROT);
        }
        let r = match self.peek(idx) {
            Some(v) => v,
            None => return self.reject(MP_ESTATE),
        };
        let fault = r.prot & kind == 0;
        match self.regions.get_mut(idx) {
            Some(Some(slot)) => {
                slot.accesses = slot.accesses.wrapping_add(1);
                if fault {
                    slot.faults = slot.faults.wrapping_add(1);
                }
            }
            _ => return self.reject(MP_ESTATE),
        }
        self.guards = self.guards.wrapping_add(1);
        if fault {
            self.segv = self.segv.wrapping_add(1);
            return self.reject(MP_ESEGV);
        }
        Ok(self.fold())
    }

    /// RELRO 收尾：只接受「RW → R」这一种形态，且只能做一次。
    pub fn relro(&mut self, idx: usize) -> Result<u64, u32> {
        if self.state == ST_FROZEN {
            return self.reject(MP_EFROZEN);
        }
        if self.state != ST_MAPPED {
            return self.reject(MP_EORDER);
        }
        let r = match self.peek(idx) {
            Some(v) => v,
            None => return self.reject(MP_ESTATE),
        };
        if r.relro_done || r.prot != (PROT_R | PROT_W) {
            return self.reject(MP_ERELRO);
        }
        match self.regions.get_mut(idx) {
            Some(Some(slot)) => {
                slot.prot = PROT_R;
                slot.relro_done = true;
                slot.switches = slot.switches.wrapping_add(1);
            }
            _ => return self.reject(MP_ESTATE),
        }
        self.relro = self.relro.wrapping_add(1);
        self.guards = self.guards.wrapping_add(1);
        Ok(self.fold())
    }

    /// 已映射区间数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 当前阶段。
    pub fn state(&self) -> u8 {
        self.state
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.state == ST_FROZEN
    }

    /// 累计拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 累计越权 SEGV 数。
    pub fn segv_count(&self) -> u32 {
        self.segv
    }

    /// 累计 RELRO 收尾数。
    pub fn relro_count(&self) -> u32 {
        self.relro
    }

    /// 累计被受理操作数（映射 / 切换 / 访问 / 封场）。
    pub fn guards(&self) -> u32 {
        self.guards
    }

    /// 区间当前权限。
    pub fn prot_at(&self, idx: usize) -> Option<u8> {
        self.peek(idx).map(|r| r.prot)
    }

    /// 区间页数。
    pub fn pages_at(&self, idx: usize) -> Option<u8> {
        self.peek(idx).map(|r| r.pages)
    }

    /// 区间成功切换次数。
    pub fn switches_at(&self, idx: usize) -> Option<u8> {
        self.peek(idx).map(|r| r.switches)
    }

    /// 区间是否已完成 RELRO。
    pub fn relro_at(&self, idx: usize) -> Option<bool> {
        self.peek(idx).map(|r| r.relro_done)
    }

    /// 全表 W^X 洁净（没有任何区间同时带 W 与 X）。
    pub fn wx_clean(&self) -> bool {
        let mut i = 0;
        while i < self.count {
            match self.regions.get(i) {
                Some(Some(r)) => {
                    if r.wx() {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// FNV-1a 指纹：区间几何 + 权限 + 全局计数（不含 root）。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h ^= self.count as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.state as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0;
        while i < REGION_CAP {
            match self.regions.get(i) {
                Some(Some(r)) => {
                    h ^= r.vaddr;
                    h = h.wrapping_mul(FNV_PRIME);
                    h ^= r.pages as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    h ^= r.prot as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    h ^= r.relro_done as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    h ^= r.switches as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                }
                _ => {}
            }
            i += 1;
        }
        h ^= self.guards as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.denies as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.segv as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.relro as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.rejects as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结：封场之后才能冻结，记录摘要作为重放基准。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.state == ST_FROZEN {
            return self.reject(MP_EFROZEN);
        }
        if self.state != ST_MAPPED {
            return self.reject(MP_EORDER);
        }
        self.state = ST_FROZEN;
        self.root = self.fold();
        Ok(self.root)
    }

    /// 重放：冻结态下重算摘要，必须与基准一致。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.state != ST_FROZEN {
            return self.reject(MP_ESTATE);
        }
        let now = self.fold();
        if now != self.root {
            return self.reject(MP_ESTATE);
        }
        Ok(now)
    }

    /// 冻结基准摘要。
    pub fn root(&self) -> u64 {
        self.root
    }
}

/// 三区样板：文本 R-X、只读 R、数据 RW，已封场。
pub fn build_three() -> ProtMap {
    let mut m = ProtMap::new();
    let _ = m.map(0x4000, 0x2000, PROT_R | PROT_X);
    let _ = m.map(0x6000, 0x1000, PROT_R);
    let _ = m.map(0x7000, 0x1000, PROT_R | PROT_W);
    let _ = m.seal();
    m
}

/// 四路独立注入：非法权限、W|X、未对齐、越权 SEGV。
pub fn inject_four() -> [bool; 4] {
    let mut a = ProtMap::new();
    let i1 = matches!(a.map(0x4000, 0x1000, PROT_NONE), Err(MP_EPROT));

    let mut b = ProtMap::new();
    let i2 = matches!(
        b.map(0x4000, 0x1000, PROT_R | PROT_W | PROT_X),
        Err(MP_EWX)
    );

    let mut c = ProtMap::new();
    let i3 = matches!(c.map(0x4800, 0x1000, PROT_R), Err(MP_EBOUNDS));

    let mut d = ProtMap::new();
    let mapped = d.map(0x4000, 0x1000, PROT_R | PROT_X).is_ok();
    let sealed = d.seal().is_ok();
    let i4 = mapped && sealed && matches!(d.access(0, ACC_W), Err(MP_ESEGV));

    [i1, i2, i3, i4]
}

// ---------------------------------------------------------------- 判据
/// 13 项静态判据：建场、映射、四类拒绝、合法迁移、SEGV、RELRO、冻结。
pub fn run_unx11254_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11254.mprotect_wx");

    let a = ProtMap::new();
    let b = ProtMap::new();
    s.add(
        "MP1-空表建场",
        a.count() == 0 && a.state() == ST_BUILD && !a.is_frozen() && a.fold() == b.fold(),
        "static",
    );

    let m = build_three();
    s.add(
        "MP2-三区映射",
        m.count() == 3
            && m.prot_at(0) == Some(PROT_R | PROT_X)
            && m.prot_at(1) == Some(PROT_R)
            && m.prot_at(2) == Some(PROT_R | PROT_W)
            && m.pages_at(0) == Some(2)
            && m.wx_clean(),
        "static",
    );

    let mut m = build_three();
    let mut fresh = ProtMap::new();
    let e1 = fresh.map(0x9000, 0x1000, PROT_NONE) == Err(MP_EPROT);
    let e2 = m.protect(0, PROT_NONE) == Err(MP_EPROT);
    let e3 = m.access(0, 0) == Err(MP_EPROT);
    s.add("MP3-权限值非法", e1 && e2 && e3, "static");

    let mut m = build_three();
    let mut fresh = ProtMap::new();
    let w1 = m.protect(0, PROT_R | PROT_W | PROT_X) == Err(MP_EWX);
    let w2 = fresh.map(0x4000, 0x1000, PROT_R | PROT_W | PROT_X) == Err(MP_EWX);
    s.add("MP4-W与X同置拒绝", w1 && w2 && m.wx_clean(), "static");

    let mut fresh = ProtMap::new();
    let b1 = fresh.map(0x4800, 0x1000, PROT_R) == Err(MP_EBOUNDS);
    let b2 = fresh.map(0x4000, 0, PROT_R) == Err(MP_EBOUNDS);
    let b3 = fresh.map(0x4000, 0x1800, PROT_R) == Err(MP_EBOUNDS);
    let b4 = fresh.map(0xffff_ffff_ffff_f000, 0x2000, PROT_R) == Err(MP_EBOUNDS);
    let b5 = fresh.map(0x4000, 0x2000, PROT_R).is_ok()
        && fresh.map(0x5000, 0x1000, PROT_R) == Err(MP_EBOUNDS);
    s.add(
        "MP5-对齐越界重叠",
        b1 && b2 && b3 && b4 && b5 && fresh.count() == 1,
        "static",
    );

    let mut empty = ProtMap::new();
    let o1 = empty.protect(0, PROT_R) == Err(MP_EORDER);
    let o2 = empty.access(0, ACC_R) == Err(MP_EORDER);
    let o3 = empty.freeze() == Err(MP_EORDER);
    let o4 = empty.relro(0) == Err(MP_EORDER);
    let mut sealed = build_three();
    let o5 = sealed.map(0x9000, 0x1000, PROT_R) == Err(MP_EORDER);
    let o6 = sealed.seal() == Err(MP_EORDER);
    s.add(
        "MP6-阶段顺序",
        o1 && o2 && o3 && o4 && o5 && o6 && empty.state() == ST_BUILD,
        "static",
    );

    let mut m = build_three();
    let t1 = m.protect(0, PROT_R).is_ok() && m.prot_at(0) == Some(PROT_R);
    let t2 = m.protect(0, PROT_R | PROT_W).is_ok() && m.prot_at(0) == Some(PROT_R | PROT_W);
    let t3 = m.protect(0, PROT_R).is_ok() && m.prot_at(0) == Some(PROT_R);
    let t4 = m.switches_at(0) == Some(3) && m.wx_clean();
    s.add(
        "MP7-经R中转三跳",
        t1 && t2 && t3 && t4,
        "static",
    );

    let mut m = build_three();
    let g1 = m.access(0, ACC_R).is_ok();
    let g2 = m.access(0, ACC_X).is_ok();
    let g3 = m.access(0, ACC_W) == Err(MP_ESEGV);
    let g4 = m.access(2, ACC_W).is_ok();
    let g5 = m.access(0, 0x8) == Err(MP_EPROT);
    let g6 = m.access(9, ACC_R) == Err(MP_ESTATE);
    s.add(
        "MP8-访问逐位判定",
        g1 && g2 && g3 && g4 && g5 && g6 && m.segv_count() == 1,
        "static",
    );

    let mut m = build_three();
    let r1 = m.relro(2).is_ok();
    let r2 = m.prot_at(2) == Some(PROT_R);
    let r3 = m.relro_at(2) == Some(true);
    let r4 = m.access(2, ACC_W) == Err(MP_ESEGV);
    s.add(
        "MP9-RELRO收尾降RW为R",
        r1 && r2 && r3 && r4 && m.relro_count() == 1 && m.wx_clean(),
        "static",
    );

    let mut m = build_three();
    let q1 = m.relro(1) == Err(MP_ERELRO);
    let q2 = m.relro(0) == Err(MP_ERELRO);
    let q3 = m.relro(2).is_ok();
    let q4 = m.relro(2) == Err(MP_ERELRO);
    let q5 = m.protect(2, PROT_R | PROT_W) == Err(MP_ERELRO);
    s.add(
        "MP10-RELRO只接受RW到R",
        q1 && q2 && q3 && q4 && q5 && m.relro_count() == 1 && m.rejects() == 4,
        "static",
    );

    let mut m = build_three();
    let ok_prot = m.protect(1, PROT_R | PROT_W).is_ok();
    let f1 = m.freeze();
    let root = match f1 {
        Ok(v) => v,
        Err(_) => 0,
    };
    let rp = m.replay();
    let f2 = m.freeze() == Err(MP_EFROZEN);
    let locked = m.protect(1, PROT_R) == Err(MP_EFROZEN);
    let root_at = m.root();
    s.add_dyn(
        "MP11-冻结重放幂等",
        ok_prot && f1.is_ok() && rp == f1 && f2 && locked && root_at == root,
        &format!("root={:x}", root),
    );

    let mut m = build_three();
    let l1 = m.access(0, ACC_X).is_ok();
    let l2 = m.protect(0, PROT_R).is_ok() && m.prot_at(0) == Some(PROT_R);
    let l3 = m.access(0, ACC_W) == Err(MP_ESEGV);
    let l4 = m.protect(0, PROT_R | PROT_W).is_ok();
    let l5 = m.access(0, ACC_W).is_ok();
    let l6 = m.protect(0, PROT_R).is_ok();
    let l7 = m.protect(0, PROT_R | PROT_X).is_ok() && m.prot_at(0) == Some(PROT_R | PROT_X);
    s.add(
        "MP12-全周期无WX同置",
        l1 && l2 && l3 && l4 && l5 && l6 && l7
            && m.wx_clean()
            && m.segv_count() == 1
            && m.switches_at(0) == Some(4),
        "static",
    );

    let inj = inject_four();
    s.add(
        "MP13-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_region_layout() {
        let m = build_three();
        assert_eq!(m.count(), 3);
        assert_eq!(m.pages_at(0), Some(2));
        assert_eq!(m.prot_at(0), Some(PROT_R | PROT_X));
        assert_eq!(m.prot_at(1), Some(PROT_R));
        assert_eq!(m.prot_at(2), Some(PROT_R | PROT_W));
        assert_eq!(m.state(), ST_MAPPED);
        assert!(m.wx_clean());
        assert_eq!(m.segv_count(), 0);
        assert_eq!(m.relro_count(), 0);
    }

    #[test]
    fn rejection_matrix() {
        let mut m = ProtMap::new();
        assert_eq!(m.map(0x4000, 0x1000, PROT_NONE), Err(MP_EPROT));
        assert_eq!(m.map(0x4000, 0x1000, PROT_R | PROT_W | PROT_X), Err(MP_EWX));
        assert_eq!(m.map(0x4800, 0x1000, PROT_R), Err(MP_EBOUNDS));
        assert_eq!(m.map(0x4000, 0x1800, PROT_R), Err(MP_EBOUNDS));
        assert_eq!(m.map(0x4000, 0x1000, PROT_R), Ok(0));
        assert_eq!(m.map(0x4000, 0x1000, PROT_R), Err(MP_EDUP));
        let mut p = 0x5000u64;
        while p <= 0x9000 {
            assert!(m.map(p, 0x1000, PROT_R).is_ok());
            p += PAGE;
        }
        assert_eq!(m.count(), REGION_CAP);
        assert_eq!(m.map(0xa000, 0x1000, PROT_R), Err(MP_EFULL));
        assert_eq!(m.rejects(), 6);
    }

    #[test]
    fn relro_flow() {
        let mut m = build_three();
        assert_eq!(m.relro(1), Err(MP_ERELRO));
        assert_eq!(m.relro(0), Err(MP_ERELRO));
        assert!(m.relro(2).is_ok());
        assert_eq!(m.prot_at(2), Some(PROT_R));
        assert_eq!(m.relro_at(2), Some(true));
        assert_eq!(m.relro(2), Err(MP_ERELRO));
        assert_eq!(m.access(2, ACC_W), Err(MP_ESEGV));
        assert_eq!(m.relro_count(), 1);
    }

    #[test]
    fn stage_and_segv() {
        let mut fresh = ProtMap::new();
        assert_eq!(fresh.access(0, ACC_R), Err(MP_EORDER));
        assert_eq!(fresh.freeze(), Err(MP_EORDER));
        let mut m = build_three();
        assert_eq!(m.access(0, ACC_X), Ok(m.fold()));
        assert_eq!(m.access(0, ACC_W), Err(MP_ESEGV));
        assert!(m.protect(0, PROT_R).is_ok());
        assert_eq!(m.access(0, ACC_X), Err(MP_ESEGV));
        assert!(m.protect(0, PROT_R | PROT_W).is_ok());
        assert_eq!(m.access(0, ACC_W), Ok(m.fold()));
        assert_eq!(m.segv_count(), 2);
        assert_eq!(m.protect(9, PROT_R), Err(MP_ESTATE));
    }

    #[test]
    fn freeze_replay_idempotent() {
        let mut m = build_three();
        let root = m.freeze();
        assert!(root.is_ok());
        assert_eq!(m.replay(), root);
        assert_eq!(m.freeze(), Err(MP_EFROZEN));
        assert_eq!(m.map(0x9000, 0x1000, PROT_R), Err(MP_EFROZEN));
        assert_eq!(m.protect(0, PROT_R), Err(MP_EFROZEN));
        assert!(m.is_frozen());
    }

    #[test]
    fn checks_green_and_injection() {
        let s = run_unx11254_checks();
        assert!(s.all_passed(), "UNX-F11254 判据未全绿: {}", s.red_items_text());
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true, true, true, true]);
    }
}
