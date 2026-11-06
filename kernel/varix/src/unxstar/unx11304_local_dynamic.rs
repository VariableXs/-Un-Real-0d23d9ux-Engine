//! UNX-F11304 local-dynamic 模型消费（0xe8f0 段）
//!
//! local-dynamic（LD）是 TLS 里“按模块取块、按偏移取变量”的两段式模型：
//! `tls_index` 只携带模块号与块内偏移，运行期先用模块号查 DTV 槽拿到本线程
//! 该模块的块基址，再加偏移得到变量地址。与 initial-exec 的固定负偏移不同，
//! LD 允许模块在运行期装载，代价是每次访问都要经过一次槽表查询。
//!
//! 模型把这段消费面拆成五道闸门，顺序固定、先命中者胜出：模块号越界、
//! 偏移越界、对齐非法、竞技场容量不足、地址算术溢出。闸门顺序本身就是规格：
//! 越界的模块号即使偏移也非法，仍报模块错误；对齐检查只在真正要分配新块时
//! 才执行，已分配槽的快路径不再复查对齐——这与装载器“块一旦成型即信任其
//! 对齐”的行为一致，也让快路径保持零分支成本。
//!
//! 本线程的 TLS 竞技场从 TCB 之后起算，块按对齐上取整顺序排布，块与块之间
//! 允许出现对齐空隙。账本记录十六个样本：十一次成功、五次拒绝，成功样本里
//! 四条走慢路径（分配）、四条走快路径（复用），拒绝样本覆盖偏移越界两次、
//! 模块越界两次、竞技场耗尽、对齐非法与算术溢出各一次。拒绝必须零副作用：
//! 槽表不改、游标不动、已分配计数不加。
//!
//! 账本登记统一用世界编号区分场景：主世界承载常规访问，满载世界用大块把
//! 512 字节竞技场撑爆，错位世界把对齐写成非 2 的幂，溢出世界把块尺寸写成
//! 接近 2^32 的值让加法回绕。四个世界互不共享状态，因此样本可以在任意顺序
//! 下重新计算而不受干扰。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe8f0;

/// 编号非法。
pub const LD_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const LD_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const LD_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const LD_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const LD_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const LD_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const LD_EBAL: u32 = SEG_BASE + 0x07;
/// 模块号越界。
pub const LD_EMOD: u32 = SEG_BASE + 0x08;
/// 偏移越界。
pub const LD_EOFF: u32 = SEG_BASE + 0x09;
/// 对齐非法。
pub const LD_EALIGN: u32 = SEG_BASE + 0x0a;
/// 竞技场满。
pub const LD_EFULL: u32 = SEG_BASE + 0x0b;
/// 算术溢出。
pub const LD_EOVER: u32 = SEG_BASE + 0x0c;
/// 票号不一致。
pub const LD_ESEAL: u32 = SEG_BASE + 0x0d;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 慢路径：本次调用分配了新块。
pub const PATH_SLOW: u8 = 1;
/// 快路径：本次调用复用已分配块。
pub const PATH_FAST: u8 = 2;

/// 模块表容量。
pub const MAX_MODS: usize = 4;
/// 竞技场容量。
pub const ARENA_CAP: u32 = 512;
/// TCB 尺寸，竞技场自 TCB 之后起算。
pub const TCB_SIZE: u32 = 64;
/// 未分配标记。
pub const UNSET: u32 = 0xffff_ffff;
/// 样本格数。
pub const SAMPLES: usize = 15;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// TLS 索引：模块号 + 块内偏移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlsIndex {
    pub module: u16,
    pub offset: u32,
}

impl TlsIndex {
    /// 构造。
    pub const fn make(module: u16, offset: u32) -> TlsIndex {
        TlsIndex { module, offset }
    }
}

/// 模块的 TLS 块描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleDesc {
    pub memsz: u32,
    pub align: u32,
}

impl ModuleDesc {
    /// 构造。
    pub const fn make(memsz: u32, align: u32) -> ModuleDesc {
        ModuleDesc { memsz, align }
    }
}

/// 2 的幂判定。
pub fn is_pow2(v: u32) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// 上取整对齐，溢出报错。
pub fn align_up(v: u32, a: u32) -> Result<u32, u32> {
    let mask = a - 1;
    match v.checked_add(mask) {
        Some(x) => Ok(x & !mask),
        None => Err(LD_EOVER),
    }
}

/// 单线程 TLS 世界：槽表、模块描述与竞技场游标。
#[derive(Clone, Copy, Debug)]
pub struct World {
    slot: [u32; MAX_MODS],
    mods: [ModuleDesc; MAX_MODS],
    nmods: u8,
    cursor: u32,
}

impl World {
    /// 空世界：槽全部未分配，游标落在 TCB 之后。
    pub const fn empty() -> World {
        World {
            slot: [UNSET; MAX_MODS],
            mods: [ModuleDesc::make(0, 1); MAX_MODS],
            nmods: 0,
            cursor: TCB_SIZE,
        }
    }

    /// 装入一个模块。
    pub fn load(&mut self, d: ModuleDesc) -> Result<(), u32> {
        if (self.nmods as usize) >= MAX_MODS {
            return Err(LD_EMOD);
        }
        match self.mods.get_mut(self.nmods as usize) {
            Some(s) => *s = d,
            None => return Err(LD_EMOD),
        }
        self.nmods += 1;
        Ok(())
    }

    /// 标准四模块：48/16、24/8、40/16、8/32。
    pub fn boot() -> World {
        let mut w = World::empty();
        let _ = w.load(ModuleDesc::make(48, 16));
        let _ = w.load(ModuleDesc::make(24, 8));
        let _ = w.load(ModuleDesc::make(40, 16));
        let _ = w.load(ModuleDesc::make(8, 32));
        w
    }

    /// 取模块描述。
    pub fn desc(&self, i: usize) -> Option<ModuleDesc> {
        if i >= self.nmods as usize {
            return None;
        }
        self.mods.get(i).copied()
    }

    /// 取模块数。
    pub fn nmods(&self) -> usize {
        self.nmods as usize
    }

    /// 取槽值。
    pub fn slot(&self, i: usize) -> Option<u32> {
        self.slot.get(i).copied()
    }

    /// 取竞技场游标。
    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    /// 已分配槽数。
    pub fn used(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < MAX_MODS {
            match self.slot.get(i) {
                Some(v) => {
                    if *v != UNSET {
                        n += 1;
                    }
                }
                None => {}
            }
            i += 1;
        }
        n
    }

    /// 慢路径入口：按模块号与偏移求变量地址。
    ///
    /// 返回 (地址, 路径)，路径 1 表示本次分配、2 表示复用。
    pub fn get(&mut self, ti: TlsIndex) -> Result<(u32, u8), u32> {
        let idx = ti.module as usize;
        let d = match self.desc(idx) {
            Some(v) => v,
            None => return Err(LD_EMOD),
        };
        if ti.offset >= d.memsz {
            return Err(LD_EOFF);
        }
        let slot = match self.slot.get(idx) {
            Some(v) => *v,
            None => return Err(LD_EMOD),
        };
        let (base, path) = if slot == UNSET {
            if !is_pow2(d.align) {
                return Err(LD_EALIGN);
            }
            let start = align_up(self.cursor, d.align)?;
            let end = match start.checked_add(d.memsz) {
                Some(v) => v,
                None => return Err(LD_EOVER),
            };
            if end > ARENA_CAP {
                return Err(LD_EFULL);
            }
            match self.slot.get_mut(idx) {
                Some(s) => *s = start,
                None => return Err(LD_EMOD),
            }
            self.cursor = end;
            (start, PATH_SLOW)
        } else {
            (slot, PATH_FAST)
        };
        let addr = match base.checked_add(ti.offset) {
            Some(v) => v,
            None => return Err(LD_EOVER),
        };
        if addr >= ARENA_CAP {
            return Err(LD_EOVER);
        }
        Ok((addr, path))
    }

    /// 块区间：从槽基址到基址 + 模块尺寸。
    pub fn block(&self, i: usize) -> Option<(u32, u32)> {
        let d = self.desc(i)?;
        let b = self.slot(i)?;
        if b == UNSET {
            return None;
        }
        match b.checked_add(d.memsz) {
            Some(e) => Some((b, e)),
            None => None,
        }
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub module: u16,
    pub offset: u32,
    pub world: u8,
    pub ok: bool,
    pub code: u32,
    pub addr: u32,
    pub path: u8,
    pub used_after: u32,
    pub cursor_after: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            module: 0,
            offset: 0,
            world: 0,
            ok: false,
            code: 0,
            addr: 0,
            path: 0,
            used_after: 0,
            cursor_after: 0,
        }
    }
}

/// FNV 折入。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    seal: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格。顺序必须严格递增，拒绝样本不得携带地址。
    pub fn register(
        &mut self,
        slot: u8,
        ti: TlsIndex,
        world: u8,
        r: Result<(u32, u8), u32>,
        used_after: u32,
        cursor_after: u32,
    ) -> Result<(), u32> {
        if self.frozen {
            return Err(LD_EFROZEN);
        }
        if (slot as usize) != self.len {
            return Err(LD_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(LD_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == slot {
                        return Err(LD_EDUP);
                    }
                }
                None => return Err(LD_ENOBS),
            }
            i += 1;
        }
        let row = match r {
            Ok((addr, path)) => {
                if path != PATH_SLOW && path != PATH_FAST {
                    return Err(LD_EMISMATCH);
                }
                if addr >= ARENA_CAP {
                    return Err(LD_EMISMATCH);
                }
                Sample {
                    slot,
                    module: ti.module,
                    offset: ti.offset,
                    world,
                    ok: true,
                    code: V_OK,
                    addr,
                    path,
                    used_after,
                    cursor_after,
                }
            }
            Err(c) => {
                if c == V_OK {
                    return Err(LD_EMISMATCH);
                }
                Sample {
                    slot,
                    module: ti.module,
                    offset: ti.offset,
                    world,
                    ok: false,
                    code: c,
                    addr: 0,
                    path: 0,
                    used_after,
                    cursor_after,
                }
            }
        };
        match self.rows.get_mut(self.len) {
            Some(s) => *s = row,
            None => return Err(LD_EID),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 取地址。
    pub fn addr(&self, i: usize) -> Option<u32> {
        match self.sample(i) {
            Some(s) => {
                if s.ok {
                    Some(s.addr)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// 成功计数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 拒绝计数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 慢路径计数。
    pub fn slow_count(&self) -> usize {
        self.path_count(PATH_SLOW)
    }

    /// 快路径计数。
    pub fn fast_count(&self) -> usize {
        self.path_count(PATH_FAST)
    }

    fn path_count(&self, p: u8) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok && s.path == p {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 某个世界最后一次登记后的游标。
    pub fn cursor_of(&self, world: u8) -> Option<u32> {
        let mut i = self.len;
        while i > 0 {
            i -= 1;
            if let Some(s) = self.rows.get(i) {
                if s.world == world {
                    return Some(s.cursor_after);
                }
            }
        }
        None
    }

    /// 某个世界最后一次登记后的槽数。
    pub fn used_of(&self, world: u8) -> Option<u32> {
        let mut i = self.len;
        while i > 0 {
            i -= 1;
            if let Some(s) = self.rows.get(i) {
                if s.world == world {
                    return Some(s.used_after);
                }
            }
        }
        None
    }

    /// 对账：格数、分布、地址轨迹、拒绝零副作用、块不重叠。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(LD_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 7 {
            return Err(LD_EBAL);
        }
        if self.slow_count() != 4 || self.fast_count() != 4 {
            return Err(LD_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(LD_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK || s.addr == 0 {
                            return Err(LD_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.path != 0 || s.addr != 0 {
                        return Err(LD_EMISMATCH);
                    }
                }
                None => return Err(LD_ENOBS),
            }
            i += 1;
        }
        if self.addr(0) != Some(64) || self.addr(1) != Some(111) {
            return Err(LD_EMISMATCH);
        }
        if self.addr(3) != Some(112) || self.addr(4) != Some(135) {
            return Err(LD_EMISMATCH);
        }
        if self.addr(5) != Some(144) || self.addr(6) != Some(196) {
            return Err(LD_EMISMATCH);
        }
        if self.addr(12) != Some(183) || self.addr(13) != Some(65) {
            return Err(LD_EMISMATCH);
        }
        let codes = [LD_EOFF, LD_EOFF, LD_EMOD, LD_EMOD, LD_EFULL, LD_EALIGN, LD_EOVER];
        let mut k = 0;
        let mut j = 0;
        while j < self.len {
            if let Some(s) = self.rows.get(j) {
                if !s.ok {
                    match codes.get(k) {
                        Some(c) => {
                            if s.code != *c {
                                return Err(LD_EMISMATCH);
                            }
                        }
                        None => return Err(LD_EBAL),
                    }
                    k += 1;
                }
            }
            j += 1;
        }
        if k != codes.len() {
            return Err(LD_EBAL);
        }
        if self.used_of(0) != Some(4) || self.cursor_of(0) != Some(200) {
            return Err(LD_EBAL);
        }
        if self.used_of(1) != Some(0) || self.cursor_of(1) != Some(TCB_SIZE) {
            return Err(LD_EBAL);
        }
        if self.used_of(2) != Some(0) || self.cursor_of(2) != Some(TCB_SIZE) {
            return Err(LD_EBAL);
        }
        if self.used_of(3) != Some(0) || self.cursor_of(3) != Some(TCB_SIZE) {
            return Err(LD_EBAL);
        }
        let mut blocks: [(u32, u32); MAX_MODS] = [(0, 0); MAX_MODS];
        let mut fresh = World::boot();
        let mut m = 0;
        while m < MAX_MODS {
            let _ = fresh.get(TlsIndex::make(m as u16, 0));
            match fresh.block(m) {
                Some(v) => blocks[m] = v,
                None => return Err(LD_ENOBS),
            }
            m += 1;
        }
        let mut q = 0;
        let mut prev_hi = TCB_SIZE;
        while q < MAX_MODS {
            let (lo, hi) = blocks[q];
            if hi <= lo || lo < prev_hi {
                return Err(LD_EBAL);
            }
            prev_hi = hi;
            q += 1;
        }
        Ok(())
    }

    /// 熔断票号。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.module as u64);
                h = fold(h, s.offset as u64);
                h = fold(h, s.world as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.addr as u64);
                h = fold(h, s.path as u64);
                h = fold(h, s.used_after as u64);
                h = fold(h, s.cursor_after as u64);
            }
            i += 1;
        }
        h
    }

    /// 冻结：首次封票，二次拒绝。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(LD_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放：重建账本并与封票比对。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(LD_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(LD_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.seal
    }

    /// 二次冻结次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 构建十五格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut w = World::boot();

    let t0 = TlsIndex::make(0, 0);
    let r0 = w.get(t0);
    let _ = l.register(0, t0, 0, r0, w.used(), w.cursor());

    let t1 = TlsIndex::make(0, 47);
    let r1 = w.get(t1);
    let _ = l.register(1, t1, 0, r1, w.used(), w.cursor());

    let t2 = TlsIndex::make(0, 48);
    let r2 = w.get(t2);
    let _ = l.register(2, t2, 0, r2, w.used(), w.cursor());

    let t3 = TlsIndex::make(1, 0);
    let r3 = w.get(t3);
    let _ = l.register(3, t3, 0, r3, w.used(), w.cursor());

    let t4 = TlsIndex::make(1, 23);
    let r4 = w.get(t4);
    let _ = l.register(4, t4, 0, r4, w.used(), w.cursor());

    let t5 = TlsIndex::make(2, 0);
    let r5 = w.get(t5);
    let _ = l.register(5, t5, 0, r5, w.used(), w.cursor());

    let t6 = TlsIndex::make(3, 4);
    let r6 = w.get(t6);
    let _ = l.register(6, t6, 0, r6, w.used(), w.cursor());

    let t7 = TlsIndex::make(3, 8);
    let r7 = w.get(t7);
    let _ = l.register(7, t7, 0, r7, w.used(), w.cursor());

    let t8 = TlsIndex::make(4, 0);
    let r8 = w.get(t8);
    let _ = l.register(8, t8, 0, r8, w.used(), w.cursor());

    let t9 = TlsIndex::make(9, 3);
    let r9 = w.get(t9);
    let _ = l.register(9, t9, 0, r9, w.used(), w.cursor());

    let mut full = World::empty();
    let _ = full.load(ModuleDesc::make(16, 8));
    let _ = full.load(ModuleDesc::make(16, 8));
    let _ = full.load(ModuleDesc::make(16, 8));
    let _ = full.load(ModuleDesc::make(500, 64));
    let ta = TlsIndex::make(3, 0);
    let ra = full.get(ta);
    let _ = l.register(10, ta, 1, ra, full.used(), full.cursor());

    let mut odd = World::empty();
    let _ = odd.load(ModuleDesc::make(16, 8));
    let _ = odd.load(ModuleDesc::make(16, 8));
    let _ = odd.load(ModuleDesc::make(16, 8));
    let _ = odd.load(ModuleDesc::make(16, 24));
    let tb = TlsIndex::make(3, 0);
    let rb = odd.get(tb);
    let _ = l.register(11, tb, 2, rb, odd.used(), odd.cursor());

    let t12 = TlsIndex::make(2, 39);
    let r12 = w.get(t12);
    let _ = l.register(12, t12, 0, r12, w.used(), w.cursor());

    let t13 = TlsIndex::make(0, 1);
    let r13 = w.get(t13);
    let _ = l.register(13, t13, 0, r13, w.used(), w.cursor());

    let mut huge = World::empty();
    let _ = huge.load(ModuleDesc::make(16, 8));
    let _ = huge.load(ModuleDesc::make(16, 8));
    let _ = huge.load(ModuleDesc::make(16, 8));
    let _ = huge.load(ModuleDesc::make(0xffff_fff0, 16));
    let tc = TlsIndex::make(3, 0);
    let rc = huge.get(tc);
    let _ = l.register(14, tc, 3, rc, huge.used(), huge.cursor());

    l
}

/// 四路注入：账本重放、快慢路径、闸门优先级、拒绝零副作用。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mut w = World::boot();
    let first = w.get(TlsIndex::make(2, 0));
    let second = w.get(TlsIndex::make(2, 39));
    out[1] = first == Ok((64, PATH_SLOW)) && second == Ok((103, PATH_FAST)) && w.used() == 1;

    let mut w2 = World::boot();
    let off_edge = w2.get(TlsIndex::make(1, 24));
    let mod_edge = w2.get(TlsIndex::make(4, 0));
    out[2] = off_edge == Err(LD_EOFF)
        && mod_edge == Err(LD_EMOD)
        && w2.get(TlsIndex::make(1, 0)) == Ok((64, PATH_SLOW));

    let mut w3 = World::boot();
    let bad = w3.get(TlsIndex::make(3, 8));
    out[3] = bad == Err(LD_EOFF)
        && w3.used() == 0
        && w3.cursor() == TCB_SIZE
        && w3.slot(3) == Some(UNSET)
        && w3.get(TlsIndex::make(0, 0)) == Ok((64, PATH_SLOW))
        && w3.cursor() == TCB_SIZE + 48;

    out
}

/// 十三项判据。
pub fn run_unx11304_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11304.localdyn");
    let l = build_ledger();

    s.add(
        "LD1-十五格齐备",
        l.len() == SAMPLES
            && !l.is_empty()
            && l.sample(0).map(|x| x.module) == Some(0)
            && l.sample(14).map(|x| x.world) == Some(3),
        "static",
    );

    s.add_dyn(
        "LD2-八允七拒分布正确",
        l.ok_count() == 8 && l.err_count() == 7 && l.audit().is_ok(),
        &format!(
            "ok={} err={} slow={} fast={}",
            l.ok_count(),
            l.err_count(),
            l.slow_count(),
            l.fast_count()
        ),
    );

    s.add(
        "LD3-慢四快四",
        l.slow_count() == 4 && l.fast_count() == 4,
        "static",
    );

    s.add(
        "LD4-主世界游标与槽位",
        l.used_of(0) == Some(4) && l.cursor_of(0) == Some(200),
        "static",
    );

    let mut w = World::boot();
    let _ = w.get(TlsIndex::make(0, 0));
    let _ = w.get(TlsIndex::make(1, 0));
    let _ = w.get(TlsIndex::make(2, 0));
    let _ = w.get(TlsIndex::make(3, 0));
    s.add(
        "LD5-块基址轨迹",
        w.block(0) == Some((64, 112))
            && w.block(1) == Some((112, 136))
            && w.block(2) == Some((144, 184))
            && w.block(3) == Some((192, 200)),
        "static",
    );

    s.add(
        "LD6-快路径偏移一致",
        l.addr(1) == l.addr(0).map(|a| a + 47)
            && l.addr(4) == Some(135)
            && l.addr(12) == Some(183)
            && l.addr(13) == Some(65),
        "static",
    );

    s.add(
        "LD7-偏移越界拒绝",
        l.sample(2).map(|x| x.code) == Some(LD_EOFF)
            && l.sample(7).map(|x| x.code) == Some(LD_EOFF)
            && World::boot().get(TlsIndex::make(0, 48)) == Err(LD_EOFF),
        "static",
    );

    s.add(
        "LD8-模块越界拒绝",
        l.sample(8).map(|x| x.code) == Some(LD_EMOD)
            && l.sample(9).map(|x| x.code) == Some(LD_EMOD)
            && World::empty().get(TlsIndex::make(0, 0)) == Err(LD_EMOD),
        "static",
    );

    s.add(
        "LD9-竞技场耗尽拒绝",
        l.sample(10).map(|x| x.code) == Some(LD_EFULL)
            && {
                let mut f = World::empty();
                let _ = f.load(ModuleDesc::make(16, 8));
                let _ = f.load(ModuleDesc::make(16, 8));
                let _ = f.load(ModuleDesc::make(16, 8));
                let _ = f.load(ModuleDesc::make(452, 64));
                f.get(TlsIndex::make(3, 0)) == Err(LD_EFULL) && f.cursor() == TCB_SIZE
            },
        "static",
    );

    s.add(
        "LD10-对齐非法拒绝",
        l.sample(11).map(|x| x.code) == Some(LD_EALIGN)
            && {
                let mut f = World::empty();
                let _ = f.load(ModuleDesc::make(16, 0));
                f.get(TlsIndex::make(0, 0)) == Err(LD_EALIGN)
                    && {
                        let mut g = World::empty();
                        let _ = g.load(ModuleDesc::make(16, 24));
                        g.get(TlsIndex::make(0, 0)) == Err(LD_EALIGN)
                    }
            },
        "static",
    );

    s.add(
        "LD11-算术溢出拒绝",
        l.sample(14).map(|x| x.code) == Some(LD_EOVER)
            && {
                let mut f = World::empty();
                let _ = f.load(ModuleDesc::make(0xffff_fff0, 16));
                f.get(TlsIndex::make(0, 0)) == Err(LD_EOVER) && f.used() == 0
            },
        "static",
    );

    s.add(
        "LD12-拒绝零副作用",
        l.sample(2).map(|x| x.used_after) == Some(1)
            && l.sample(2).map(|x| x.cursor_after) == Some(112)
            && l.sample(7).map(|x| x.used_after) == Some(4)
            && l.sample(7).map(|x| x.cursor_after) == Some(200)
            && l.sample(10).map(|x| x.used_after) == Some(0)
            && l.sample(11).map(|x| x.cursor_after) == Some(TCB_SIZE)
            && l.sample(14).map(|x| x.cursor_after) == Some(TCB_SIZE),
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(LD_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "LD13-冻结重放与四路注入",
        root != 0 && rep == Ok(root) && again && f.root() == root && f.rejects() == 1 && inj == [true; 4],
        &format!("root={:016x} inject={:?}", root, inj),
    );

    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!(
        "rows={} ok={} err={} slow={} fast={}",
        l.len(),
        l.ok_count(),
        l.err_count(),
        l.slow_count(),
        l.fast_count()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_layout_is_deterministic() {
        let mut w = World::boot();
        assert_eq!(w.get(TlsIndex::make(0, 0)), Ok((64, PATH_SLOW)));
        assert_eq!(w.get(TlsIndex::make(1, 0)), Ok((112, PATH_SLOW)));
        assert_eq!(w.get(TlsIndex::make(2, 0)), Ok((144, PATH_SLOW)));
        assert_eq!(w.get(TlsIndex::make(3, 0)), Ok((192, PATH_SLOW)));
        assert_eq!(w.cursor(), 200);
        assert_eq!(w.used(), 4);
    }

    #[test]
    fn fast_path_reuses_block() {
        let mut w = World::boot();
        let a = w.get(TlsIndex::make(0, 3));
        let b = w.get(TlsIndex::make(0, 3));
        assert_eq!(a, Ok((67, PATH_SLOW)));
        assert_eq!(b, Ok((67, PATH_FAST)));
        assert_eq!(w.used(), 1);
    }

    #[test]
    fn offset_at_memsz_is_denied() {
        let mut w = World::boot();
        assert_eq!(w.get(TlsIndex::make(3, 8)), Err(LD_EOFF));
        assert_eq!(w.get(TlsIndex::make(3, 7)), Ok((71, PATH_SLOW)));
    }

    #[test]
    fn unknown_module_is_denied() {
        let mut w = World::boot();
        assert_eq!(w.get(TlsIndex::make(4, 0)), Err(LD_EMOD));
        assert_eq!(w.get(TlsIndex::make(65535, 0)), Err(LD_EMOD));
        assert_eq!(w.used(), 0);
    }

    #[test]
    fn arena_full_leaves_state_intact() {
        let mut w = World::empty();
        let _ = w.load(ModuleDesc::make(464, 64));
        assert_eq!(w.get(TlsIndex::make(0, 0)), Err(LD_EFULL));
        assert_eq!(w.cursor(), TCB_SIZE);
        assert_eq!(w.used(), 0);
    }

    #[test]
    fn misaligned_module_is_denied() {
        let mut w = World::empty();
        let _ = w.load(ModuleDesc::make(16, 12));
        assert_eq!(w.get(TlsIndex::make(0, 0)), Err(LD_EALIGN));
        let mut z = World::empty();
        let _ = z.load(ModuleDesc::make(16, 0));
        assert_eq!(z.get(TlsIndex::make(0, 0)), Err(LD_EALIGN));
    }

    #[test]
    fn huge_memsz_overflows() {
        let mut w = World::empty();
        let _ = w.load(ModuleDesc::make(0xffff_ffff, 16));
        assert_eq!(w.get(TlsIndex::make(0, 0)), Err(LD_EOVER));
        assert_eq!(w.used(), 0);
    }

    #[test]
    fn ledger_seals_and_replays() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(LD_EFROZEN));
        assert_eq!(l.rejects(), 1);
        assert!(l.is_frozen());
    }

    #[test]
    fn denials_do_not_allocate() {
        let mut w = World::boot();
        let _ = w.get(TlsIndex::make(0, 0));
        let before = (w.used(), w.cursor(), w.slot(1));
        assert_eq!(w.get(TlsIndex::make(1, 24)), Err(LD_EOFF));
        assert_eq!((w.used(), w.cursor(), w.slot(1)), before);
        assert_eq!(w.get(TlsIndex::make(1, 0)), Ok((112, PATH_SLOW)));
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11304_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
