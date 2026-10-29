//! unx4085_dlookpath — UNX-F4085 lookup 命中/未命中路径（诊断段 0x8CC0）。
//!
//! 判据原文：UNX-F4085-J1 命中/未命中/未命中后建立三路径各 1,000 次行为正确，
//!           未命中穿透 fs 层次数与理论值一致（缓存生效断言）。
//! 可验性质（本模块自证面）：
//!   J1 三路径各 1,000 次：命中（Hit）/ 未命中并在 fs 侧建起（MissCreated）/
//!      未命中且 fs 侧确无（MissAbsent），三路计数各自 1,000，互不串账；
//!   J2 穿透层次数账：每次 miss 恰好穿透 fs 一层（fs_probes 增 1），
//!      全命中负载下 fs_probes 增量为 0——缓存生效的硬指标；
//!   J3 负向查找避免：MissAbsent 后登记负项，同名二次查找走 Negative 出口，
//!      不再穿透 fs（neg_hits 增 1、fs_probes 不变）；
//!   J4 总账守恒：lookups == hits + created + absent + neg_hits；
//!      fs_probes == created + absent（一未命中一穿透，无重复穿透无漏穿透）；
//!   J5 非法输入全拒：空名（E_NAME_EMPTY）、名超长（E_NAME_LONG）、
//!      负项重复登记（E_DUP_NEG），拒收不进 lookups 账。
//! 设计纪律：
//!   1. fs 侧用 FakeFs 线性表模拟（样本千条量级），每次 lookup 计一次 probe，
//!      probe 数是"穿透层次"的唯一度量，别处不许再自加。
//!   2. 缓存项三态：Positive（带 ino）/ Negative（确认不存在）/ Empty（占位未用）。
//!      负项是 dentry 的合法形态（dcache 负项），不是错误，故 Negative 出口返回 Ok。
//!   3. 建项一律"先查重后插入"：正项按 (parent,name) 唯一，负项同样唯一，
//!      否则同一名字会同时存在正负两态（自相矛盾）。
//!   4. 失效接口只清项不删项（state 置 Empty），保证下标稳定、链不搬动。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8CC0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8CC0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NAME_EMPTY: u16 = SEG_BASE + 1; // 空名字
const E_NAME_LONG: u16 = SEG_BASE + 2; // 名字超长（NAME_MAX=255）
const E_DUP_NEG: u16 = SEG_BASE + 3; // 同一键重复登记负项
const E_DUP_POS: u16 = SEG_BASE + 4; // 同一键重复登记正项（fs 侧重名）
const E_BAL: u16 = SEG_BASE + 5; // 查找/穿透账不平
const E_NO_KEY: u16 = SEG_BASE + 6; // 失效不存在的键
const E_FS_BAD: u16 = SEG_BASE + 7; // fs 表项自坏（ino=0）

/// 名字长度上限（Linux NAME_MAX）。
pub const NAME_MAX: usize = 255;

/// 缓存项态：正项 / 负项 / 空位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LookupState {
    Empty = 0,    // 占位未用（失效后回此态）
    Positive = 1, // 确认存在，带 ino
    Negative = 2, // 确认不存在（负项）
}

/// lookup 三路径出口（判据 J1 的口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LookupOutcome {
    Hit(u64),         // 缓存命中正项
    MissCreated(u64), // 未命中，fs 侧查到并建项
    MissAbsent,       // 未命中，fs 侧确无（本次建负项）
    Negative,         // 负项命中（不再穿透）
}

/// fs 表项：目录项实态。
pub struct FsEntry {
    pub parent: u64, // 父目录 ino
    pub name: Vec<u8>, // 名字
    pub ino: u64,    // 子项 ino
}

/// 假 fs：线性表 + 穿透计数（probe 是唯一穿透度量）。
pub struct FakeFs {
    pub entries: Vec<FsEntry>, // 目录项表
    pub probes: u32,           // 被探测次数（每 lookup 未命中 +1）
}

impl FakeFs {
    pub fn new() -> FakeFs {
        FakeFs {
            entries: Vec::new(),
            probes: 0,
        }
    }

    /// 登记目录项：ino=0 拒（保留号）、同 (parent,name) 重名拒。
    pub fn add(&mut self, parent: u64, name: &[u8], ino: u64) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if ino == 0 {
            return Err(E_FS_BAD);
        }
        if self.probe_ro(parent, name).is_some() {
            return Err(E_DUP_POS);
        }
        self.entries.push(FsEntry {
            parent,
            name: copy(name),
            ino,
        });
        Ok(())
    }

    /// 只读查（不计 probe）。
    pub fn probe_ro(&self, parent: u64, name: &[u8]) -> Option<u64> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.parent == parent && bytes_eq(&e.name, name) {
                return Some(e.ino);
            }
            i += 1;
        }
        None
    }

    /// 真探测：计一次 probe，再查表。
    pub fn probe(&mut self, parent: u64, name: &[u8]) -> Option<u64> {
        self.probes += 1;
        self.probe_ro(parent, name)
    }

    /// fs 侧直改（模拟外部挂载写入）：删除条目，供失效联动使用。
    pub fn remove(&mut self, parent: u64, name: &[u8]) -> Result<u64, u16> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].parent == parent && bytes_eq(&self.entries[i].name, name) {
                let ino = self.entries[i].ino;
                self.entries.remove(i);
                return Ok(ino);
            }
            i += 1;
        }
        Err(E_NO_KEY)
    }
}

/// 缓存项：键 + 态 + ino。
pub struct CacheEntry {
    pub parent: u64,     // 父目录 ino
    pub name: Vec<u8>,   // 名字
    pub state: LookupState, // 态
    pub ino: u64,        // 正项的 ino（负项为 0）
}

/// 查找统计账。
pub struct LookupStat {
    pub lookups: u32,  // 总查找次数（只计合法入参）
    pub hits: u32,     // 正项命中
    pub created: u32,  // 未命中后建正项
    pub absent: u32,   // 未命中且 fs 侧确无
    pub neg_hits: u32, // 负项命中
    pub fs_probes: u32, // 穿透 fs 次数（应等于 created + absent）
    pub invalidated: u32, // 失效次数
}

impl LookupStat {
    fn new() -> LookupStat {
        LookupStat {
            lookups: 0,
            hits: 0,
            created: 0,
            absent: 0,
            neg_hits: 0,
            fs_probes: 0,
            invalidated: 0,
        }
    }

    /// J4 守恒：三路出口之和 == 查找次数。
    pub fn conserved(&self) -> bool {
        self.lookups == self.hits + self.created + self.absent + self.neg_hits
    }

    /// 穿透账：一未命中一穿透。
    pub fn probe_conserved(&self) -> bool {
        self.fs_probes == self.created + self.absent
    }
}

/// 查找枢纽：fs + 缓存表 + 统计。
pub struct LookupPath {
    pub fs: FakeFs,             // 实态
    pub cache: Vec<CacheEntry>, // 目录项缓存
    pub stat: LookupStat,       // 统计账
}

impl LookupPath {
    pub fn new() -> LookupPath {
        LookupPath {
            fs: FakeFs::new(),
            cache: Vec::new(),
            stat: LookupStat::new(),
        }
    }

    /// 缓存定位（只读）：返回下标。
    pub fn find(&self, parent: u64, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.cache.len() {
            if self.cache[i].parent == parent
                && bytes_eq(&self.cache[i].name, name)
                && self.cache[i].state != LookupState::Empty
            {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 建正项：先查重（同键不得已有正/负项）。
    fn put_positive(&mut self, parent: u64, name: &[u8], ino: u64) -> Result<(), u16> {
        if self.find(parent, name).is_some() {
            return Err(E_DUP_POS);
        }
        self.cache.push(CacheEntry {
            parent,
            name: copy(name),
            state: LookupState::Positive,
            ino,
        });
        Ok(())
    }

    /// 建负项：同上查重（同一键不得同时有正项）。
    fn put_negative(&mut self, parent: u64, name: &[u8]) -> Result<(), u16> {
        if self.find(parent, name).is_some() {
            return Err(E_DUP_NEG);
        }
        self.cache.push(CacheEntry {
            parent,
            name: copy(name),
            state: LookupState::Negative,
            ino: 0,
        });
        Ok(())
    }

    /// 三路径查找（J1/J2/J3 的唯一入口）。
    pub fn lookup(&mut self, parent: u64, name: &[u8]) -> Result<LookupOutcome, u16> {
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if name.len() > NAME_MAX {
            return Err(E_NAME_LONG);
        }
        self.stat.lookups += 1;
        // ① 缓存先看
        if let Some(i) = self.find(parent, name) {
            match self.cache[i].state {
                LookupState::Positive => {
                    self.stat.hits += 1;
                    return Ok(LookupOutcome::Hit(self.cache[i].ino));
                }
                LookupState::Negative => {
                    self.stat.neg_hits += 1;
                    return Ok(LookupOutcome::Negative); // 负项命中：零穿透
                }
                LookupState::Empty => {}
            }
        }
        // ② 穿透 fs 一层
        let got = self.fs.probe(parent, name);
        self.stat.fs_probes += 1;
        match got {
            Some(ino) => {
                self.put_positive(parent, name, ino)?;
                self.stat.created += 1;
                Ok(LookupOutcome::MissCreated(ino))
            }
            None => {
                self.put_negative(parent, name)?;
                self.stat.absent += 1;
                Ok(LookupOutcome::MissAbsent)
            }
        }
    }

    /// 失效：条目态置 Empty（不搬下标），负项与正项同样可失效。
    pub fn invalidate(&mut self, parent: u64, name: &[u8]) -> Result<(), u16> {
        let i = match self.find(parent, name) {
            Some(x) => x,
            None => return Err(E_NO_KEY),
        };
        self.cache[i].state = LookupState::Empty;
        self.cache[i].ino = 0;
        self.stat.invalidated += 1;
        Ok(())
    }

    /// 缓存条目数（非空项）。
    pub fn live_entries(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.cache.len() {
            if self.cache[i].state != LookupState::Empty {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 字节比较（无 slice 相等糖以外依赖）。
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 字节拷贝。
fn copy(name: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < name.len() {
        v.push(name[i]);
        i += 1;
    }
    v
}

/// 造名字：固定前缀 + 十进制序号，保证千条样本互不相同。
fn name_of(i: u32, out: &mut Vec<u8>) {
    out.clear();
    out.push(b'f');
    let mut digits: [u8; 10] = [0; 10];
    let mut n = 0usize;
    let mut v = i;
    if v == 0 {
        out.push(b'0');
        return;
    }
    while v > 0 && n < 10 {
        digits[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        out.push(digits[n]);
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NAME_EMPTY => "空名字（dentry 名至少 1 字节）",
        E_NAME_LONG => "名字超长（NAME_MAX=255）",
        E_DUP_NEG => "同一键重复登记负项",
        E_DUP_POS => "同一键重复登记正项（fs 侧重名）",
        E_BAL => "查找/穿透账不平",
        E_NO_KEY => "失效不存在的键",
        E_FS_BAD => "fs 表项自坏（ino=0 保留号）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：三路径各千次 → 穿透账 → 负项避免 → 失效联动 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut lp = LookupPath::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut i = 0u32;

    // R1：fs 侧先备 1,000 条真实目录项（父按 i/100 分十组）。
    while i < 1_000 {
        name_of(i, &mut buf);
        lp.fs.add(i as u64 / 100, &buf, 0x2000 + i as u64)?;
        i += 1;
    }
    if lp.fs.entries.len() != 1_000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：第一轮 1,000 次查找——全部 MissCreated（未命中后建立），穿透恰 1,000 层。
    i = 0;
    while i < 1_000 {
        name_of(i, &mut buf);
        match lp.lookup(i as u64 / 100, &buf)? {
            LookupOutcome::MissCreated(v) => {
                if v != 0x2000 + i as u64 {
                    return Err(E_BAL);
                }
            }
            _ => return Err(E_BAL),
        }
        i += 1;
    }
    if lp.stat.created != 1_000 || lp.stat.fs_probes != 1_000 {
        return Err(E_BAL);
    }
    if lp.cache.len() != 1_000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：第二轮同名 1,000 次——全部 Hit，穿透增量为 0（缓存生效断言 J2）。
    let probes_before = lp.stat.fs_probes;
    i = 0;
    while i < 1_000 {
        name_of(i, &mut buf);
        match lp.lookup(i as u64 / 100, &buf)? {
            LookupOutcome::Hit(v) => {
                if v != 0x2000 + i as u64 {
                    return Err(E_BAL);
                }
            }
            _ => return Err(E_BAL),
        }
        i += 1;
    }
    if lp.stat.hits != 1_000 {
        return Err(E_BAL);
    }
    if lp.stat.fs_probes != probes_before {
        return Err(E_BAL); // 全命中负载下零穿透
    }
    hits += 1;

    // R4：1,000 次负向查找——首轮 MissAbsent（建负项），次轮 Negative（零穿透，J3）。
    i = 0;
    while i < 1_000 {
        name_of(0x4000 + i, &mut buf);
        if lp.lookup(9, &buf)? != LookupOutcome::MissAbsent {
            return Err(E_BAL);
        }
        i += 1;
    }
    if lp.stat.absent != 1_000 {
        return Err(E_BAL);
    }
    let probes_neg = lp.stat.fs_probes;
    i = 0;
    while i < 1_000 {
        name_of(0x4000 + i, &mut buf);
        if lp.lookup(9, &buf)? != LookupOutcome::Negative {
            return Err(E_BAL);
        }
        i += 1;
    }
    if lp.stat.neg_hits != 1_000 {
        return Err(E_BAL);
    }
    if lp.stat.fs_probes != probes_neg {
        return Err(E_BAL); // 负项命中不得再穿透
    }
    hits += 1;

    // R5：守恒终审——4,000 次查找四路各 1,000，穿透 2,000（J4）。
    if lp.stat.lookups != 4_000 {
        return Err(E_BAL);
    }
    if !lp.stat.conserved() {
        return Err(E_BAL);
    }
    if !lp.stat.probe_conserved() || lp.stat.fs_probes != 2_000 {
        return Err(E_BAL);
    }
    if lp.live_entries() != 2_000 {
        return Err(E_BAL); // 1,000 正项 + 1,000 负项
    }
    hits += 1;

    // R6：失效联动——失效第 7 条后重查必须再穿透一次（缓存不再生效）。
    name_of(7, &mut buf);
    lp.invalidate(0, &buf)?;
    let p2 = lp.stat.fs_probes;
    match lp.lookup(0, &buf)? {
        LookupOutcome::MissCreated(v) => {
            if v != 0x2007 {
                return Err(E_BAL);
            }
        }
        _ => return Err(E_BAL),
    }
    if lp.stat.fs_probes != p2 + 1 || lp.stat.invalidated != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：空名 → E_NAME_EMPTY，且不进 lookups 账。
    let lk_before = lp.stat.lookups;
    if lp.lookup(0, b"") != Err(E_NAME_EMPTY) {
        return Err(E_NAME_EMPTY);
    }
    if lp.stat.lookups != lk_before {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：超长名（256 字节）→ E_NAME_LONG。
    let mut longbuf: Vec<u8> = Vec::new();
    let mut z = 0usize;
    while z < NAME_MAX + 1 {
        longbuf.push(b'z');
        z += 1;
    }
    if lp.lookup(0, &longbuf) != Err(E_NAME_LONG) {
        return Err(E_NAME_LONG);
    }
    hits += 1;

    // I3：失效不存在的键 → E_NO_KEY（负项也算存在，故用一个从未查过的名字）。
    if lp.invalidate(0, b"never-looked") != Err(E_NO_KEY) {
        return Err(E_NO_KEY);
    }
    hits += 1;

    // I4：fs 侧重名登记 → E_DUP_POS；ino=0 → E_FS_BAD（两路都不得入表）。
    let sz = lp.fs.entries.len();
    name_of(1, &mut buf);
    if lp.fs.add(0, &buf, 0x9999) != Err(E_DUP_POS) {
        return Err(E_DUP_POS);
    }
    if lp.fs.add(3, b"zeroino", 0) != Err(E_FS_BAD) {
        return Err(E_FS_BAD);
    }
    if lp.fs.entries.len() != sz {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：缓存内部重项注入——已有正项再建负项必须被拒（正负不得共存）。
    name_of(5, &mut buf);
    if lp.put_negative(0, &buf) != Err(E_DUP_NEG) {
        return Err(E_DUP_NEG);
    }
    hits += 1;

    // 终审：账平 + 穿透平 + 缓存项数与正负分账对得上。
    if !lp.stat.conserved() || !lp.stat.probe_conserved() {
        return Err(E_BAL);
    }
    if lp.live_entries() != 2_000 {
        return Err(E_BAL); // 2,000：R6 失效一条后重建一条，净额不变
    }
    hits += 1;

    Ok(hits)
}
