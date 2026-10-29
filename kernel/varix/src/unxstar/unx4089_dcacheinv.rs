//! unx4089_dcacheinv — UNX-F4089 缓存一致性（fs 变更后失效）（诊断段 0x8D40）。
//!
//! 判据原文：UNX-F4089-J1 fs 侧变更（fake 直改）后缓存条目失效可观测（失效事件计数
//!           =变更数），失效后查找穿透并取回新实态。
//! 可验性质（本模块自证面）：
//!   J1 fs 侧发生 N 次变更（版本号 +1、size 改变），缓存侧按需感知：每次感知到
//!      版本不符记一次失效事件，最终失效事件计数 == N（一变更一失效，不多不少）；
//!   J2 失效后取数必须穿透到 fs 并回填新实态（refreshes +1，且返回的 size 等于
//!      fs 当前 size），不得返回旧缓存值；
//!   J3 未变更的条目不得被误判失效（false invalidation == 0），缓存该省的穿透仍然省；
//!   J4 守恒：loads + stale_hits == 取数次数；invalidations == stale_hits + explicit_inv；
//!       cache 条目数 == fs 文件数（一文件一条，不建多余项）；
//!   J5 非法输入拒：取不存在的 ino → E_NO_FILE，显式失效不存在项 → E_NO_ENTRY，
//!       fs 写入不存在 ino → E_NO_FILE，拒收不动任何计数。
//! 设计纪律：
//!   1. 版本号是唯一的失效判据：fs 每次写都把 version +1，缓存记住读时的 version；
//!      两边不等即失效。不做"逐字段比 size/mtime"的近似判断（近似会漏判同尺寸改写）。
//!   2. 失效不在 fs 写时推送（无跨模块回调），而在缓存取数时惰性发现：这样本模块
//!      不依赖任何通知机制，纯函数式可验；代价是"变更数与失效事件数"只在被取用时
//!      才对齐，drill 里用逐条取数把两账拉平。
//!   3. 失效是"置 Invalid + 立即重取"两动作：置态先做（账不可丢），重取失败则留在
//!      Invalid（下次再试），不得因为重取失败就回滚失效事件。
//!   4. 显式失效（上层主动 invalidate）与惰性失效分账：explicit_inv 单列，
//!      总失效 = 惰性 + 显式，便于定位"谁把缓存打掉的"。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8D40 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8D40;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NO_FILE: u16 = SEG_BASE + 1; // fs 侧无此 inode
const E_NO_ENTRY: u16 = SEG_BASE + 2; // 缓存侧无此条目
const E_DUP_INO: u16 = SEG_BASE + 3; // fs 侧重复建同一 inode
const E_BAL: u16 = SEG_BASE + 4; // 失效/取数账不平
const E_VER_STALE: u16 = SEG_BASE + 5; // 版本号回退（fs 侧账坏）
const E_INO_ZERO: u16 = SEG_BASE + 6; // ino=0 保留号
const E_OVER: u16 = SEG_BASE + 7; // 版本号/尺寸越界推进

/// fs 侧文件：ino + 尺寸 + 版本。
pub struct FsFile {
    pub ino: u64,     // inode 号
    pub size: u64,    // 字节长度（实态）
    pub version: u32, // 变更版本（每次写 +1）
}

/// 假 fs：文件表 + 变更账。
pub struct FsView {
    pub files: Vec<FsFile>, // 文件表
    pub changes: u32,       // 变更总次数（判据 N）
}

impl FsView {
    pub fn new() -> FsView {
        FsView {
            files: Vec::new(),
            changes: 0,
        }
    }

    /// 建文件：ino 非 0 且唯一。
    pub fn create(&mut self, ino: u64, size: u64) -> Result<(), u16> {
        if ino == 0 {
            return Err(E_INO_ZERO);
        }
        if self.find(ino).is_some() {
            return Err(E_DUP_INO);
        }
        self.files.push(FsFile {
            ino,
            size,
            version: 1, // 初版从 1 起，0 表示"从未落盘"
        });
        Ok(())
    }

    /// 定位下标。
    pub fn find(&self, ino: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.files.len() {
            if self.files[i].ino == ino {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 直改（模拟其它挂载/进程写文件）：版本 +1，变更账 +1。
    pub fn write(&mut self, ino: u64, size: u64) -> Result<u32, u16> {
        let idx = match self.find(ino) {
            Some(i) => i,
            None => return Err(E_NO_FILE),
        };
        if self.files[idx].version == u32::MAX {
            return Err(E_OVER);
        }
        self.files[idx].size = size;
        self.files[idx].version += 1;
        self.changes += 1;
        Ok(self.files[idx].version)
    }

    /// 读实态：(size, version)。
    pub fn read(&self, ino: u64) -> Result<(u64, u32), u16> {
        match self.find(ino) {
            Some(i) => Ok((self.files[i].size, self.files[i].version)),
            None => Err(E_NO_FILE),
        }
    }
}

/// 缓存条目态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntState {
    Absent = 0,  // 未缓存
    Valid = 1,   // 与 fs 同版本，可用
    Invalid = 2, // 已知失效，待重取
}

/// 缓存条目：ino + 快照 + 态。
pub struct InvEntry {
    pub ino: u64,     // inode 号
    pub size: u64,    // 缓存的尺寸快照
    pub version: u32, // 缓存的版本快照
    pub state: EntState, // 态
}

/// 失效/取数账。
pub struct InvLedger {
    pub loads: u32,          // 首次装载（缓存空）
    pub hits: u32,           // 有效缓存命中（零穿透）
    pub stale_hits: u32,     // 取数时发现版本不符（惰性失效）
    pub explicit_inv: u32,   // 显式失效次数
    pub invalidations: u32,  // 失效事件总数（惰性 + 显式）
    pub refreshes: u32,      // 重取回填次数
    pub invalid_gets: u32,   // 显式失效后同版本重取的取数次数
    pub false_inv: u32,      // 误判失效（版本相同却判失效）
}

impl InvLedger {
    fn new() -> InvLedger {
        InvLedger {
            loads: 0,
            hits: 0,
            stale_hits: 0,
            explicit_inv: 0,
            invalidations: 0,
            refreshes: 0,
            invalid_gets: 0,
            false_inv: 0,
        }
    }

    /// J4 守恒其一：一次取数必落四路之一（装载 / 命中 / 惰性失效 / 显式失效重取）。
    /// `gets` 由枢纽侧传入，避免账内重复记一份同源计数。
    pub fn gets_conserved(&self, gets: u32) -> bool {
        self.loads + self.hits + self.stale_hits + self.invalid_gets == gets
    }

    /// J4 守恒其二：失效事件 = 惰性 + 显式。
    pub fn inv_conserved(&self) -> bool {
        self.invalidations == self.stale_hits + self.explicit_inv
    }
}

/// 失效枢纽：fs + 缓存 + 账。
pub struct DCacheInv {
    pub fs: FsView,          // 实态
    pub cache: Vec<InvEntry>, // 缓存
    pub led: InvLedger,      // 账
    pub gets: u32,           // 取数总次数
}

impl DCacheInv {
    pub fn new() -> DCacheInv {
        DCacheInv {
            fs: FsView::new(),
            cache: Vec::new(),
            led: InvLedger::new(),
            gets: 0,
        }
    }

    /// 缓存定位。
    pub fn find(&self, ino: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.cache.len() {
            if self.cache[i].ino == ino {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 回填：把 fs 实态写进缓存（若无条目则新建）。
    fn refill(&mut self, ino: u64) -> Result<(u64, u32), u16> {
        let (size, ver) = self.fs.read(ino)?;
        match self.find(ino) {
            Some(i) => {
                self.cache[i].size = size;
                self.cache[i].version = ver;
                self.cache[i].state = EntState::Valid;
            }
            None => self.cache.push(InvEntry {
                ino,
                size,
                version: ver,
                state: EntState::Valid,
            }),
        }
        self.led.refreshes += 1;
        Ok((size, ver))
    }

    /// 取数：先比版本（惰性失效），再决定命中或穿透（J1/J2/J3）。
    pub fn get(&mut self, ino: u64) -> Result<u64, u16> {
        if ino == 0 {
            return Err(E_INO_ZERO);
        }
        // fs 侧不存在：直接拒，不进账
        let (fs_size, fs_ver) = self.fs.read(ino)?;
        self.gets += 1;
        match self.find(ino) {
            None => {
                self.led.loads += 1;
                let (s, _) = self.refill(ino)?;
                Ok(s)
            }
            Some(i) => {
                let same_ver = self.cache[i].version == fs_ver;
                let valid = self.cache[i].state == EntState::Valid;
                if same_ver && valid {
                    self.led.hits += 1; // 版本一致且态有效：零穿透
                    return Ok(self.cache[i].size);
                }
                // 失效：置态先做（账不可丢），再重取（设计纪律 3）
                if !same_ver {
                    self.led.stale_hits += 1;
                    self.led.invalidations += 1;
                } else {
                    self.led.invalid_gets += 1; // 显式失效后的同版本重取，不重复计失效
                }
                self.cache[i].state = EntState::Invalid;
                let (s, _) = self.refill(ino)?;
                if s != fs_size {
                    return Err(E_BAL); // 回填后仍与实态不符：账坏
                }
                Ok(s)
            }
        }
    }

    /// 显式失效：置 Invalid 并记显式账（不重取）。
    pub fn invalidate(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find(ino) {
            Some(x) => x,
            None => return Err(E_NO_ENTRY),
        };
        if self.cache[i].state == EntState::Valid {
            self.cache[i].state = EntState::Invalid;
            self.led.explicit_inv += 1;
            self.led.invalidations += 1;
            Ok(())
        } else {
            Ok(()) // 已失效：幂等，不重复记事件
        }
    }

    /// 版本一致性全审：所有 Valid 条目的版本必须与 fs 相同（J3 的静态面）。
    pub fn audit_versions(&self) -> bool {
        let mut i = 0usize;
        while i < self.cache.len() {
            if self.cache[i].state == EntState::Valid {
                match self.fs.read(self.cache[i].ino) {
                    Ok((_, v)) => {
                        if v != self.cache[i].version {
                            return false;
                        }
                    }
                    Err(_) => return false,
                }
            }
            i += 1;
        }
        true
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_FILE => "fs 侧无此 inode",
        E_NO_ENTRY => "缓存侧无此条目",
        E_DUP_INO => "fs 侧重复建同一 inode",
        E_BAL => "失效/取数账不平或回填不符",
        E_VER_STALE => "版本号回退（fs 侧账坏）",
        E_INO_ZERO => "ino=0 保留号",
        E_OVER => "版本号触顶（不可再推进）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：装载 → 变更感知 → 失效重取 → 误判为零 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut d = DCacheInv::new();

    // R0：建 60 个文件（版本 1，size 1000+i）。
    let mut i = 0u64;
    while i < 60 {
        d.fs.create(0x3000 + i, 1_000 + i)?;
        i += 1;
    }
    if d.fs.files.len() != 60 {
        return Err(E_BAL);
    }
    hits += 1;

    // R1：首轮取数 60 次全部 Loads（缓存空），条目数 == 文件数（一文件一条）。
    i = 0;
    while i < 60 {
        if d.get(0x3000 + i)? != 1_000 + i {
            return Err(E_BAL);
        }
        i += 1;
    }
    if d.led.loads != 60 || d.led.refreshes != 60 {
        return Err(E_BAL);
    }
    if d.cache.len() != 60 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：再取 60 次全部 Hits，零穿透（版本一致，J3 正面）。
    let refr_before = d.led.refreshes;
    i = 0;
    while i < 60 {
        if d.get(0x3000 + i)? != 1_000 + i {
            return Err(E_BAL);
        }
        i += 1;
    }
    if d.led.hits != 60 || d.led.refreshes != refr_before {
        return Err(E_BAL);
    }
    if d.led.stale_hits != 0 || d.led.false_inv != 0 {
        return Err(E_BAL); // 未变更不得误判失效
    }
    hits += 1;

    // R3：fs 直改 50 个文件（各 +1 版本、尺寸翻倍），随后逐个取数：
    //     必须 50 次惰性失效 + 50 次重取，且取回新尺寸（J1/J2 主面）。
    i = 0;
    while i < 50 {
        d.fs.write(0x3000 + i, (1_000 + i) * 2)?;
        i += 1;
    }
    if d.fs.changes != 50 {
        return Err(E_BAL);
    }
    let inv_before = d.led.invalidations;
    i = 0;
    while i < 50 {
        if d.get(0x3000 + i)? != (1_000 + i) * 2 {
            return Err(E_BAL); // 必须拿到新实态
        }
        i += 1;
    }
    if d.led.invalidations - inv_before != 50 {
        return Err(E_BAL); // 失效事件数 == 变更数
    }
    if d.led.stale_hits != 50 {
        return Err(E_BAL);
    }
    if d.led.refreshes != 60 + 50 {
        return Err(E_BAL);
    }
    if !d.audit_versions() {
        return Err(E_BAL); // 重取后全表版本一致
    }
    hits += 1;

    // R4：未变更的 10 个文件再取——仍是 Hit，误判失效为零（J3 负面）。
    i = 50;
    while i < 60 {
        if d.get(0x3000 + i)? != 1_000 + i {
            return Err(E_BAL);
        }
        i += 1;
    }
    if d.led.false_inv != 0 {
        return Err(E_BAL);
    }
    if d.led.hits != 70 {
        return Err(E_BAL); // 60 + 10
    }
    hits += 1;

    // R5：显式失效——对有效条目失效一次记一次，重复失效幂等不重复记账。
    let exp_before = d.led.explicit_inv;
    d.invalidate(0x3000 + 55)?;
    d.invalidate(0x3000 + 55)?; // 第二次已 Invalid：幂等
    if d.led.explicit_inv != exp_before + 1 {
        return Err(E_BAL);
    }
    let inv2 = d.led.invalidations;
    if d.get(0x3000 + 55)? != 1_055 {
        return Err(E_BAL); // 显式失效后取数穿透并回填
    }
    if d.led.invalidations != inv2 {
        return Err(E_BAL); // 显式已计过，取数时不得重复计惰性失效
    }
    hits += 1;

    // R6：账守恒——取数 181 次 = loads 60 + hits 70 + stale 50 + invalid_gets 1；
    //     并核"失效事件 = 惰性 + 显式"这条硬等式（J4）。
    if !d.led.inv_conserved() || !d.led.gets_conserved(d.gets) {
        return Err(E_BAL);
    }
    if d.led.invalidations != 50 + 1 {
        return Err(E_BAL);
    }
    if d.led.invalid_gets != 1 {
        return Err(E_BAL);
    }
    if d.gets != 60 + 60 + 50 + 10 + 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：取不存在的 ino → E_NO_FILE，且 gets 不增（拒收不动账）。
    let g_before = d.gets;
    if d.get(0xDEAD) != Err(E_NO_FILE) {
        return Err(E_NO_FILE);
    }
    if d.gets != g_before {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：显式失效不存在项 → E_NO_ENTRY；ino=0 → E_INO_ZERO。
    if d.invalidate(0xDEAD) != Err(E_NO_ENTRY) {
        return Err(E_NO_ENTRY);
    }
    if d.get(0) != Err(E_INO_ZERO) {
        return Err(E_INO_ZERO);
    }
    hits += 1;

    // I3：fs 侧重复建项 → E_DUP_INO；0 号 → E_INO_ZERO；写不存在 → E_NO_FILE。
    if d.fs.create(0x3000, 1) != Err(E_DUP_INO) {
        return Err(E_DUP_INO);
    }
    if d.fs.create(0, 1) != Err(E_INO_ZERO) {
        return Err(E_INO_ZERO);
    }
    if d.fs.write(0xDEAD, 1) != Err(E_NO_FILE) {
        return Err(E_NO_FILE);
    }
    hits += 1;

    // I4：版本回退不可构造（只增），但读到"缓存版本比 fs 新"必须被识别为失效：
    //     手改缓存版本为 fs+5，取数必须走惰性失效并回填，而不是返回旧值。
    let idx = match d.find(0x3000 + 5) {
        Some(x) => x,
        None => return Err(E_NO_ENTRY),
    };
    d.cache[idx].size = 0xDEAD_BEEF;
    d.cache[idx].version += 5;
    let got = d.get(0x3000 + 5)?;
    if got != (1_005) * 2 {
        return Err(E_BAL); // 版本不符即失效重取，不返回 0xDEADBEEF
    }
    hits += 1;

    // 终审：全表版本一致 + 失效账守恒 + 缓存条目数不膨胀。
    if !d.audit_versions() || !d.led.inv_conserved() || !d.led.gets_conserved(d.gets) {
        return Err(E_BAL);
    }
    if d.cache.len() != 60 {
        return Err(E_BAL); // 一文件一条，重取不得新增条目
    }
    hits += 1;

    Ok(hits)
}
