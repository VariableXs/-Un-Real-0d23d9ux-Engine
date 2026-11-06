//! UNX-F11305 __tls_get_addr 与 DTV 管理（0xe900 段）
//!
//! DTV（Dynamic Thread Vector）是 TLS 的二级索引：第 0 项是代际计数器，
//! 第 m 项是模块 m 在本线程的 TLS 块基址。`__tls_get_addr` 拿到 `tls_index`
//! 后先按模块号定位 DTV 槽，槽为空则向本线程竞技场申请块并惰性初始化，
//! 槽非空则直接原路返回——前者是慢路径，后者是快路径。容量按 1、2、4、8
//! 几何增长，且只在访问越过当前容量时才增长，避免小线程为高位模块付全表代价。
//!
//! 代际（generation）解决的是“槽表被复用”的问题：线程销毁后槽表可能被
//! 下一线程继承，块基址属于旧竞技场，必须视为无效。模型把代际写进每一项，
//! 重置代际后旧项一律视为未初始化，访问时按慢路径重新申请；被重置的竞技场
//! 从 TCB 之后重新起算，因此重建后的地址序列与首次成型完全一致——这条
//! “重建同构”正是本模型要证明的核心不变量。
//!
//! 惰性重建是逐槽的：只补被访问过的模块，未访问的模块保持失效，直到真正
//! 被访问。账本十六格记录该行为的完整轨迹：前九格建表（含三次扩容）、
//! 三格拒绝（模块越界、偏移越界、代际失效），后四格在重置代际与竞技场之后
//! 逐个重建前四个模块，地址与首次成型逐一同值。
//!
//! 闸门顺序固定：模块号越界 → 偏移越界 → 容量增长 → 槽查询/分配。模块号
//! 越界先于偏移判定，因为偏移的合法上界来自模块描述本身；容量增长先于槽
//! 查询，否则高位模型会被误判为空槽。拒绝路径零副作用：不扩容、不分配、
//! 不动游标，账本以“拒绝后计数不变”作为可验证据。
//!
//! 压力面用千线程同构验证：每个线程独立建表，按旋转顺序做三轮回访，
//! 全部必须命中快路径且地址与基准布局吻合；任一错位即计数加一。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe900;

/// 编号非法。
pub const DT_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const DT_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const DT_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const DT_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const DT_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const DT_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const DT_EBAL: u32 = SEG_BASE + 0x07;
/// 模块号越界。
pub const DT_EMOD: u32 = SEG_BASE + 0x08;
/// 偏移越界。
pub const DT_EOFF: u32 = SEG_BASE + 0x09;
/// 对齐非法。
pub const DT_EALIGN: u32 = SEG_BASE + 0x0a;
/// 容量耗尽。
pub const DT_EFULL: u32 = SEG_BASE + 0x0b;
/// 算术溢出。
pub const DT_EOVER: u32 = SEG_BASE + 0x0c;
/// 代际失效。
pub const DT_ESTALE: u32 = SEG_BASE + 0x0d;
/// 票号不一致。
pub const DT_ESEAL: u32 = SEG_BASE + 0x0e;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 慢路径：本次分配。
pub const PATH_SLOW: u8 = 1;
/// 快路径：复用槽值。
pub const PATH_FAST: u8 = 2;

/// DTV 初始容量。
pub const DTV_INIT_CAP: usize = 1;
/// DTV 最大容量。
pub const DTV_MAX_CAP: usize = 8;
/// 模块表容量。
pub const MODS_WIDE: usize = 9;
/// 竞技场容量。
pub const ARENA_CAP: u32 = 1024;
/// 竞技场起点（TCB 之后）。
pub const ARENA_START: u32 = 64;
/// 未分配标记。
pub const UNSET: u32 = 0xffff_ffff;
/// 样本格数。
pub const SAMPLES: usize = 16;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// TLS 索引。
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
        None => Err(DT_EOVER),
    }
}

/// 单线程 TLS 竞技场。
#[derive(Clone, Copy, Debug)]
pub struct Arena {
    cursor: u32,
    cap: u32,
    last: u32,
    allocs: u32,
}

impl Arena {
    /// 新建：游标落在 TCB 之后。
    pub const fn new() -> Arena {
        Arena {
            cursor: ARENA_START,
            cap: ARENA_CAP,
            last: UNSET,
            allocs: 0,
        }
    }

    /// 申请一块，返回块基址。
    pub fn alloc(&mut self, memsz: u32, align: u32) -> Result<u32, u32> {
        if !is_pow2(align) {
            return Err(DT_EALIGN);
        }
        let start = align_up(self.cursor, align)?;
        let end = match start.checked_add(memsz) {
            Some(v) => v,
            None => return Err(DT_EOVER),
        };
        if end > self.cap {
            return Err(DT_EFULL);
        }
        self.cursor = end;
        self.last = start;
        self.allocs += 1;
        Ok(start)
    }

    /// 重建：游标归位，历史分配计数清零。
    pub fn reset(&mut self) {
        self.cursor = ARENA_START;
        self.last = UNSET;
        self.allocs = 0;
    }

    /// 游标。
    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    /// 最近一次分配的基址。
    pub fn last(&self) -> u32 {
        self.last
    }

    /// 分配次数。
    pub fn allocs(&self) -> u32 {
        self.allocs
    }
}

/// 动态线程向量。
#[derive(Clone, Copy, Debug)]
pub struct Dtv {
    base: [u32; DTV_MAX_CAP],
    gen: [u16; DTV_MAX_CAP],
    cap: usize,
    grows: u16,
    allocs: u32,
    epoch: u16,
}

impl Dtv {
    /// 新建：容量 1，代际 1。
    pub const fn new() -> Dtv {
        Dtv {
            base: [UNSET; DTV_MAX_CAP],
            gen: [0; DTV_MAX_CAP],
            cap: DTV_INIT_CAP,
            grows: 0,
            allocs: 0,
            epoch: 1,
        }
    }

    /// 几何扩容到至少 need 项。
    fn grow(&mut self, need: usize) -> Result<(), u32> {
        if need <= self.cap {
            return Ok(());
        }
        let mut cap = self.cap;
        while cap < need {
            let next = match cap.checked_mul(2) {
                Some(v) => v,
                None => return Err(DT_EFULL),
            };
            if next > DTV_MAX_CAP {
                return Err(DT_EFULL);
            }
            cap = next;
            self.grows += 1;
            self.cap = cap;
        }
        Ok(())
    }

    /// 解析一次访问，返回 (地址, 路径)。
    pub fn resolve(
        &mut self,
        ti: TlsIndex,
        mods: &[ModuleDesc],
        arena: &mut Arena,
    ) -> Result<(u32, u8), u32> {
        let idx = ti.module as usize;
        let d = match mods.get(idx) {
            Some(v) => *v,
            None => return Err(DT_EMOD),
        };
        if ti.offset >= d.memsz {
            return Err(DT_EOFF);
        }
        if idx >= self.cap {
            self.grow(idx + 1)?;
        }
        let entry = match self.base.get(idx) {
            Some(v) => *v,
            None => return Err(DT_EFULL),
        };
        let g = match self.gen.get(idx) {
            Some(v) => *v,
            None => return Err(DT_EFULL),
        };
        if entry != UNSET && g == self.epoch {
            let addr = match entry.checked_add(ti.offset) {
                Some(v) => v,
                None => return Err(DT_EOVER),
            };
            return Ok((addr, PATH_FAST));
        }
        let base = arena.alloc(d.memsz, d.align)?;
        match self.base.get_mut(idx) {
            Some(s) => *s = base,
            None => return Err(DT_EFULL),
        }
        match self.gen.get_mut(idx) {
            Some(s) => *s = self.epoch,
            None => return Err(DT_EFULL),
        }
        self.allocs += 1;
        let addr = match base.checked_add(ti.offset) {
            Some(v) => v,
            None => return Err(DT_EOVER),
        };
        Ok((addr, PATH_SLOW))
    }

    /// 只读观测一格：未初始化或代际失效均报失效。
    pub fn peek(&self, idx: usize) -> Result<(u32, u16), u32> {
        if idx >= self.cap {
            return Err(DT_ENOBS);
        }
        let b = match self.base.get(idx) {
            Some(v) => *v,
            None => return Err(DT_ENOBS),
        };
        let g = match self.gen.get(idx) {
            Some(v) => *v,
            None => return Err(DT_ENOBS),
        };
        if b == UNSET || g != self.epoch {
            return Err(DT_ESTALE);
        }
        Ok((b, g))
    }

    /// 重置代际：旧项全部失效。
    pub fn reset_epoch(&mut self) -> u16 {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.epoch = 1;
        }
        self.epoch
    }

    /// 容量。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 扩容次数。
    pub fn grows(&self) -> u16 {
        self.grows
    }

    /// 惰性分配次数。
    pub fn allocs(&self) -> u32 {
        self.allocs
    }

    /// 代际。
    pub fn epoch(&self) -> u16 {
        self.epoch
    }

    /// 当前代际下有效的槽数。
    pub fn live(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.cap {
            match self.peek(i) {
                Ok(_) => n += 1,
                Err(_) => {}
            }
            i += 1;
        }
        n
    }

    /// 槽值（含失效项）。
    pub fn slot(&self, i: usize) -> Option<u32> {
        self.base.get(i).copied()
    }
}

/// 一个完整的单线程夹具：向量 + 竞技场 + 模块表。
#[derive(Clone, Copy, Debug)]
pub struct Fixture {
    pub dtv: Dtv,
    pub arena: Arena,
    mods: [ModuleDesc; MODS_WIDE],
    nmods: u8,
}

impl Fixture {
    /// 空夹具。
    pub const fn new() -> Fixture {
        Fixture {
            dtv: Dtv::new(),
            arena: Arena::new(),
            mods: [ModuleDesc::make(0, 1); MODS_WIDE],
            nmods: 0,
        }
    }

    /// 装入模块。
    pub fn load(&mut self, d: ModuleDesc) -> Result<(), u32> {
        if (self.nmods as usize) >= MODS_WIDE {
            return Err(DT_EFULL);
        }
        match self.mods.get_mut(self.nmods as usize) {
            Some(s) => *s = d,
            None => return Err(DT_EFULL),
        }
        self.nmods += 1;
        Ok(())
    }

    /// 标准五模块：16/16、32/32、8/8、24/16、8/8。
    pub fn wide() -> Fixture {
        let mut f = Fixture::new();
        let _ = f.load(ModuleDesc::make(16, 16));
        let _ = f.load(ModuleDesc::make(32, 32));
        let _ = f.load(ModuleDesc::make(8, 8));
        let _ = f.load(ModuleDesc::make(24, 16));
        let _ = f.load(ModuleDesc::make(8, 8));
        f
    }

    /// 宽模块表：九个 8/8 模块，用于逼出容量上限。
    pub fn nine() -> Fixture {
        let mut f = Fixture::new();
        let mut i = 0;
        while i < MODS_WIDE {
            let _ = f.load(ModuleDesc::make(8, 8));
            i += 1;
        }
        f
    }

    /// 模块数。
    pub fn nmods(&self) -> usize {
        self.nmods as usize
    }

    /// 一次访问。
    pub fn get(&mut self, m: u16, off: u32) -> Result<(u32, u8), u32> {
        self.dtv
            .resolve(TlsIndex::make(m, off), &self.mods[..self.nmods as usize], &mut self.arena)
    }

    /// 容量。
    pub fn cap(&self) -> usize {
        self.dtv.cap()
    }

    /// 扩容次数。
    pub fn grows(&self) -> u16 {
        self.dtv.grows()
    }

    /// 分配次数。
    pub fn allocs(&self) -> u32 {
        self.dtv.allocs()
    }

    /// 游标。
    pub fn cursor(&self) -> u32 {
        self.arena.cursor()
    }
}

/// 压力面的标准四模块。
pub const THREAD_MODS: [ModuleDesc; 4] = [
    ModuleDesc::make(48, 16),
    ModuleDesc::make(24, 8),
    ModuleDesc::make(40, 16),
    ModuleDesc::make(8, 32),
];

/// 一次成型得到的布局。
pub fn thread_layout() -> [u32; 4] {
    let mut d = Dtv::new();
    let mut a = Arena::new();
    let mut out = [0u32; 4];
    let mut m = 0;
    while m < 4 {
        match d.resolve(TlsIndex::make(m as u16, 0), &THREAD_MODS, &mut a) {
            Ok((addr, _)) => out[m] = addr,
            Err(_) => out[m] = UNSET,
        }
        m += 1;
    }
    out
}

/// 千线程同构压力：返回错位次数。
pub fn stress(threads: u32) -> u32 {
    let want = thread_layout();
    let mut bad = 0u32;
    let mut t = 0u32;
    while t < threads {
        let mut d = Dtv::new();
        let mut a = Arena::new();
        let mut m = 0u32;
        while m < 4 {
            let _ = d.resolve(TlsIndex::make(m as u16, 0), &THREAD_MODS, &mut a);
            m += 1;
        }
        let mut r = 0u32;
        while r < 3 {
            let mut k = 0u32;
            while k < 4 {
                let m = (k + r) % 4;
                let off = (t + m * 7) % THREAD_MODS[m as usize].memsz;
                match d.resolve(TlsIndex::make(m as u16, off), &THREAD_MODS, &mut a) {
                    Ok((addr, PATH_FAST)) => {
                        if want[m as usize] == UNSET || addr != want[m as usize] + off {
                            bad += 1;
                        }
                    }
                    _ => bad += 1,
                }
                k += 1;
            }
            r += 1;
        }
        t += 1;
    }
    bad
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
    pub cap: u8,
    pub grows: u8,
    pub allocs: u8,
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
            cap: 0,
            grows: 0,
            allocs: 0,
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

    /// 登记一格。
    pub fn register(
        &mut self,
        slot: u8,
        ti: TlsIndex,
        world: u8,
        r: Result<(u32, u8), u32>,
        cap: u8,
        grows: u8,
        allocs: u8,
    ) -> Result<(), u32> {
        if self.frozen {
            return Err(DT_EFROZEN);
        }
        if (slot as usize) != self.len {
            return Err(DT_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(DT_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == slot {
                        return Err(DT_EDUP);
                    }
                }
                None => return Err(DT_ENOBS),
            }
            i += 1;
        }
        let row = match r {
            Ok((addr, path)) => {
                if path != PATH_SLOW && path != PATH_FAST {
                    return Err(DT_EMISMATCH);
                }
                if addr >= ARENA_CAP {
                    return Err(DT_EMISMATCH);
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
                    cap,
                    grows,
                    allocs,
                }
            }
            Err(c) => {
                if c == V_OK {
                    return Err(DT_EMISMATCH);
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
                    cap,
                    grows,
                    allocs,
                }
            }
        };
        match self.rows.get_mut(self.len) {
            Some(s) => *s = row,
            None => return Err(DT_EID),
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

    /// 对账：格数、分布、容量轨迹、地址轨迹、拒绝零副作用。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(DT_EBAL);
        }
        if self.ok_count() != 13 || self.err_count() != 3 {
            return Err(DT_EBAL);
        }
        if self.slow_count() != 9 || self.fast_count() != 4 {
            return Err(DT_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(DT_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK || s.addr == 0 {
                            return Err(DT_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.path != 0 || s.addr != 0 {
                        return Err(DT_EMISMATCH);
                    }
                }
                None => return Err(DT_ENOBS),
            }
            i += 1;
        }
        let caps = [1u8, 1, 2, 2, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8];
        let grows = [0u8, 0, 1, 1, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3];
        let mut j = 0;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => {
                    match caps.get(j) {
                        Some(c) => {
                            if s.cap != *c {
                                return Err(DT_EMISMATCH);
                            }
                        }
                        None => return Err(DT_EBAL),
                    }
                    match grows.get(j) {
                        Some(g) => {
                            if s.grows != *g {
                                return Err(DT_EMISMATCH);
                            }
                        }
                        None => return Err(DT_EBAL),
                    }
                }
                None => return Err(DT_ENOBS),
            }
            j += 1;
        }
        let bases = [64u32, 96, 128, 144, 168];
        let slot_idx = [0usize, 2, 4, 6, 7];
        let mut k = 0;
        while k < bases.len() {
            let s = slot_idx[k];
            if self.addr(s) != bases.get(k).copied() {
                return Err(DT_EMISMATCH);
            }
            k += 1;
        }
        if self.addr(1) != Some(79)
            || self.addr(3) != Some(127)
            || self.addr(5) != Some(135)
            || self.addr(8) != Some(175)
        {
            return Err(DT_EMISMATCH);
        }
        if self.sample(9).map(|x| x.code) != Some(DT_EMOD) {
            return Err(DT_EMISMATCH);
        }
        if self.sample(10).map(|x| x.code) != Some(DT_EOFF) {
            return Err(DT_EMISMATCH);
        }
        if self.sample(11).map(|x| x.code) != Some(DT_ESTALE) {
            return Err(DT_EMISMATCH);
        }
        if self.sample(0).map(|x| x.allocs) != Some(1)
            || self.sample(7).map(|x| x.allocs) != Some(5)
            || self.sample(12).map(|x| x.allocs) != Some(6)
            || self.sample(15).map(|x| x.allocs) != Some(9)
        {
            return Err(DT_EMISMATCH);
        }
        let mut look = 12;
        let mut first = 0;
        while look < 16 {
            if self.addr(look) != self.addr(first) {
                return Err(DT_EMISMATCH);
            }
            look += 1;
            first += 2;
        }
        let ends = [80u32, 128, 136, 168, 176];
        let mut prev_hi = ARENA_START;
        let mut q = 0;
        while q < bases.len() {
            let lo = bases[q];
            let hi = ends[q];
            if hi <= lo || lo < prev_hi {
                return Err(DT_EBAL);
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
                h = fold(h, s.cap as u64);
                h = fold(h, s.grows as u64);
                h = fold(h, s.allocs as u64);
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DT_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(DT_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(DT_ESEAL);
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

/// 构建十六格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut f = Fixture::wide();

    let push = |l: &mut Ledger, f: &mut Fixture, m: u16, off: u32, world: u8, slot: u8| {
        let r = f.get(m, off);
        let _ = l.register(
            slot,
            TlsIndex::make(m, off),
            world,
            r,
            f.cap() as u8,
            f.grows() as u8,
            f.allocs() as u8,
        );
    };

    push(&mut l, &mut f, 0, 0, 0, 0);
    push(&mut l, &mut f, 0, 15, 0, 1);
    push(&mut l, &mut f, 1, 0, 0, 2);
    push(&mut l, &mut f, 1, 31, 0, 3);
    push(&mut l, &mut f, 2, 0, 0, 4);
    push(&mut l, &mut f, 2, 7, 0, 5);
    push(&mut l, &mut f, 3, 0, 0, 6);
    push(&mut l, &mut f, 4, 0, 0, 7);
    push(&mut l, &mut f, 4, 7, 0, 8);
    push(&mut l, &mut f, 5, 0, 1, 9);
    push(&mut l, &mut f, 3, 24, 1, 10);

    let _ = f.dtv.reset_epoch();
    let stale = match f.dtv.peek(0) {
        Ok(_) => Ok((0u32, 0u8)),
        Err(c) => Err(c),
    };
    let _ = l.register(
        11,
        TlsIndex::make(0, 0),
        1,
        stale,
        f.cap() as u8,
        f.grows() as u8,
        f.allocs() as u8,
    );

    f.arena.reset();
    push(&mut l, &mut f, 0, 0, 2, 12);
    push(&mut l, &mut f, 1, 0, 2, 13);
    push(&mut l, &mut f, 2, 0, 2, 14);
    push(&mut l, &mut f, 3, 0, 2, 15);

    l
}

/// 四路注入：账本重放、快慢路径、扩容上限、拒绝零副作用。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mut f = Fixture::wide();
    let first = f.get(0, 0);
    let second = f.get(0, 0);
    out[1] = first == Ok((64, PATH_SLOW))
        && second == Ok((64, PATH_FAST))
        && f.dtv.peek(0) == Ok((64, 1))
        && f.dtv.live() == 1
        && f.arena.allocs() == 1;

    let mut g = Fixture::wide();
    let high = g.get(4, 0);
    out[2] = high == Ok((64, PATH_SLOW)) && g.cap() == DTV_MAX_CAP && g.grows() == 3 && g.get(4, 0) == Ok((64, PATH_FAST));

    let mut h = Fixture::nine();
    let over_cap = h.get(8, 0);
    let mut m = Fixture::new();
    let _ = m.load(ModuleDesc::make(16, 12));
    let bad_align = m.get(0, 0);
    let mut n = Fixture::wide();
    out[3] = over_cap == Err(DT_EFULL)
        && h.cap() == DTV_MAX_CAP
        && h.allocs() == 0
        && h.cursor() == ARENA_START
        && bad_align == Err(DT_EALIGN)
        && m.arena.allocs() == 0
        && n.get(5, 0) == Err(DT_EMOD)
        && n.allocs() == 0
        && n.cursor() == ARENA_START;

    out
}

/// 十三项判据。
pub fn run_unx11305_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11305.dtv");
    let l = build_ledger();

    s.add(
        "DT1-十六格齐备",
        l.len() == SAMPLES
            && !l.is_empty()
            && l.sample(0).map(|x| x.module) == Some(0)
            && l.sample(15).map(|x| x.world) == Some(2),
        "static",
    );

    s.add_dyn(
        "DT2-十三允三拒分布正确",
        l.ok_count() == 13 && l.err_count() == 3 && l.audit().is_ok(),
        &format!(
            "ok={} err={} slow={} fast={}",
            l.ok_count(),
            l.err_count(),
            l.slow_count(),
            l.fast_count()
        ),
    );

    s.add(
        "DT3-慢九快四",
        l.slow_count() == 9 && l.fast_count() == 4,
        "static",
    );

    s.add(
        "DT4-容量几何增长",
        l.sample(0).map(|x| x.cap) == Some(1)
            && l.sample(2).map(|x| x.cap) == Some(2)
            && l.sample(4).map(|x| x.cap) == Some(4)
            && l.sample(7).map(|x| x.cap) == Some(8)
            && l.sample(15).map(|x| x.grows) == Some(3),
        "static",
    );

    s.add(
        "DT5-基址轨迹",
        l.addr(0) == Some(64)
            && l.addr(2) == Some(96)
            && l.addr(4) == Some(128)
            && l.addr(6) == Some(144)
            && l.addr(7) == Some(168),
        "static",
    );

    s.add(
        "DT6-快路径偏移一致",
        l.addr(1) == Some(79)
            && l.addr(3) == Some(127)
            && l.addr(5) == Some(135)
            && l.addr(8) == Some(175),
        "static",
    );

    s.add(
        "DT7-模块与偏移拒绝",
        l.sample(9).map(|x| x.code) == Some(DT_EMOD)
            && l.sample(10).map(|x| x.code) == Some(DT_EOFF)
            && Fixture::wide().get(9, 0) == Err(DT_EMOD)
            && Fixture::wide().get(3, 24) == Err(DT_EOFF),
        "static",
    );

    s.add(
        "DT8-代际失效与再武装",
        l.sample(11).map(|x| x.code) == Some(DT_ESTALE)
            && {
                let mut f = Fixture::wide();
                let _ = f.get(0, 0);
                let before = f.dtv.peek(0);
                let _ = f.dtv.reset_epoch();
                let after = f.dtv.peek(0);
                let again = f.get(0, 0);
                before == Ok((64, 1)) && after == Err(DT_ESTALE) && again == Ok((80, PATH_SLOW))
            },
        "static",
    );

    s.add(
        "DT9-重建同构且惰性",
        l.addr(12) == l.addr(0)
            && l.addr(13) == l.addr(2)
            && l.addr(14) == l.addr(4)
            && l.addr(15) == l.addr(6)
            && l.sample(12).map(|x| x.path) == Some(PATH_SLOW),
        "static",
    );

    s.add(
        "DT10-容量上限拒绝",
        {
            let mut f = Fixture::nine();
            f.get(8, 0) == Err(DT_EFULL) && f.cap() == DTV_MAX_CAP && f.allocs() == 0
        },
        "static",
    );

    s.add(
        "DT11-对齐非法零副作用",
        {
            let mut f = Fixture::new();
            let _ = f.load(ModuleDesc::make(16, 12));
            let a = f.get(0, 0);
            let mut g = Fixture::new();
            let _ = g.load(ModuleDesc::make(16, 0));
            a == Err(DT_EALIGN)
                && f.allocs() == 0
                && f.cursor() == ARENA_START
                && g.get(0, 0) == Err(DT_EALIGN)
        },
        "static",
    );

    s.add_dyn(
        "DT12-千线程压力零错位",
        stress(1000) == 0 && thread_layout() == [64, 112, 144, 192],
        &format!("bad={} layout={:?}", stress(1000), thread_layout()),
    );

    let mut g = build_ledger();
    let rep = g.replay();
    let root = match g.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = g.freeze() == Err(DT_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "DT13-冻结重放与四路注入",
        root != 0 && rep == Ok(root) && again && g.root() == root && g.rejects() == 1 && inj == [true; 4],
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
    fn lazy_alloc_then_fast_path() {
        let mut f = Fixture::wide();
        assert_eq!(f.get(0, 0), Ok((64, PATH_SLOW)));
        assert_eq!(f.get(0, 0), Ok((64, PATH_FAST)));
        assert_eq!(f.allocs(), 1);
        assert_eq!(f.cursor(), 80);
    }

    #[test]
    fn capacity_grows_geometrically() {
        let mut f = Fixture::wide();
        assert_eq!(f.cap(), 1);
        let _ = f.get(1, 0);
        assert_eq!(f.cap(), 2);
        let _ = f.get(2, 0);
        assert_eq!(f.cap(), 4);
        let _ = f.get(4, 0);
        assert_eq!(f.cap(), 8);
        assert_eq!(f.grows(), 3);
    }

    #[test]
    fn wide_table_denies_ninth_module() {
        let mut f = Fixture::nine();
        assert_eq!(f.get(8, 0), Err(DT_EFULL));
        assert_eq!(f.cap(), DTV_MAX_CAP);
        assert_eq!(f.allocs(), 0);
        assert_eq!(f.cursor(), ARENA_START);
    }

    #[test]
    fn offset_at_memsz_is_denied() {
        let mut f = Fixture::wide();
        assert_eq!(f.get(2, 8), Err(DT_EOFF));
        assert_eq!(f.get(2, 7), Ok((71, PATH_SLOW)));
    }

    #[test]
    fn epoch_bump_invalidates_slots() {
        let mut f = Fixture::wide();
        let _ = f.get(0, 0);
        assert_eq!(f.dtv.peek(0), Ok((64, 1)));
        let e = f.dtv.reset_epoch();
        assert_eq!(e, 2);
        assert_eq!(f.dtv.peek(0), Err(DT_ESTALE));
        assert_eq!(f.dtv.live(), 0);
        f.arena.reset();
        assert_eq!(f.get(0, 0), Ok((64, PATH_SLOW)));
    }

    #[test]
    fn rebuild_reproduces_layout() {
        let mut f = Fixture::wide();
        let mut first = [0u32; 4];
        let mut m = 0;
        while m < 4 {
            match f.get(m as u16, 0) {
                Ok((a, _)) => first[m] = a,
                Err(_) => first[m] = UNSET,
            }
            m += 1;
        }
        let _ = f.dtv.reset_epoch();
        f.arena.reset();
        let mut m2 = 0;
        while m2 < 4 {
            assert_eq!(f.get(m2 as u16, 0), Ok((first[m2], PATH_SLOW)));
            m2 += 1;
        }
    }

    #[test]
    fn stress_is_aligned_over_thousand_threads() {
        assert_eq!(thread_layout(), [64, 112, 144, 192]);
        assert_eq!(stress(1000), 0);
    }

    #[test]
    fn ledger_seals_and_replays() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(DT_EFROZEN));
        assert_eq!(l.rejects(), 1);
        assert!(l.is_frozen());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11305_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
