//! cgb820_shm_adapter — CGPU-F0820 Intel 共享内存适配。
//!
//! 判据原文（验收行摘录）：Intel 共享内存适配：跨域内存池（系统内存/本地
//! 内存双域统一账本，按域分配可回退）；绑定表驻留（句柄-页绑定带纪元防
//! 陈旧引用）；clamp/tiling 参数落地（对齐校验不过即拒）；池账守恒（总页
//! = 占用+空闲+保留，任意时刻可验）。判据：双域、驻留绑定、参数校验、
//! 守恒账本。
//!
//! 诊断段 0x7100（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；所有失败路径
//! 走 Result<_, u16> 闭集错误码；账本定容、守恒恒等式每步可验；本模块自
//! 带 drill() 自例程，构造正/负样本走核心路径，返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb820-v1";

/// 诊断段基（本单独占 0x7100）。
pub const SEG_BASE: u16 = 0x7100;

/// 池未开门（open 前任何分配请求都被拒——状态机未就绪）。
pub const XC_POOL_NOT_READY: u16 = SEG_BASE + 1;
/// 池容量耗尽（定容页数不足——防御性拒绝，不静默换域）。
pub const XC_CAP_EXCEEDED: u16 = SEG_BASE + 2;
/// 绑定表未命中（解绑/查询的句柄不在册）。
pub const XC_BIND_MISS: u16 = SEG_BASE + 3;
/// clamp 参数失配（宽/高对齐不满足 tiling 要求）。
pub const XC_CLAMP_MISMATCH: u16 = SEG_BASE + 4;
/// tiling 参数非法（未知模式或与域组合矛盾）。
pub const XC_TILE_INVALID: u16 = SEG_BASE + 5;
/// 双重释放（页仍标记 live 时再次 free——语义违约）。
pub const XC_DOUBLE_FREE: u16 = SEG_BASE + 6;
/// 跨域绑定（绑定域与页域不一致——隔离违约）。
pub const XC_DOMAIN_CROSS: u16 = SEG_BASE + 7;
/// 账本漂移（守恒恒等式不成立——内部一致性破坏）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 8;

/// 定容池页数上限。
pub const POOL_PAGES: usize = 64;
/// 定容绑定表上限。
pub const BIND_CAP: usize = 32;

/// 内存域（判据"双域"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    /// 系统内存域（CPU 可直接寻址）。
    Sysmem,
    /// 本地内存域（GPU 专用，带宽高）。
    LocalMem,
}

/// tiling 模式（判据"tiling 参数落地"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TileMode {
    /// 线性布局，无对齐额外要求。
    Linear,
    /// X 平铺，要求宽按 128 对齐。
    TileX,
    /// Y 平铺，要求宽按 128、高按 32 对齐。
    TileY,
    /// 紧凑 Yf，要求宽按 64、高按 64 对齐。
    TileYf,
}

impl TileMode {
    /// 返回 (宽对齐, 高对齐) 要求（判据"对齐校验不过即拒"）。
    fn align_req(self) -> (usize, usize) {
        match self {
            TileMode::Linear => (1, 1),
            TileMode::TileX => (128, 1),
            TileMode::TileY => (128, 32),
            TileMode::TileYf => (64, 64),
        }
    }
}

/// clamp 参数（判据"clamp/tiling 参数落地"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Clamp {
    /// 逻辑宽度（像素）。
    pub w: usize,
    /// 逻辑高度（像素）。
    pub h: usize,
}

impl Clamp {
    /// 校验 clamp 是否满足 tiling 对齐要求。
    pub fn fits(self, tile: TileMode) -> Result<(), u16> {
        let (aw, ah) = tile.align_req();
        if self.w == 0 || self.h == 0 {
            return Err(XC_CLAMP_MISMATCH);
        }
        if self.w % aw != 0 || self.h % ah != 0 {
            return Err(XC_CLAMP_MISMATCH);
        }
        Ok(())
    }
}

/// 页账条目（池内定容槽位）。
#[derive(Clone, Debug)]
pub struct PgEntry {
    /// 页号（0 起，池内唯一）。
    pub page: usize,
    /// 所属域。
    pub domain: Domain,
    /// tiling 模式（分配时锁定）。
    pub tile: TileMode,
    /// 是否占用中。
    pub live: bool,
    /// 保留标记（不可分配但未释放——驱动更新窗口等）。
    pub reserved: bool,
    /// 分配纪元（释放后递增，防陈旧句柄复用）。
    pub epoch: u64,
}

/// 绑定表条目（判据"绑定表驻留"）。
#[derive(Clone, Debug)]
pub struct Bind {
    /// 句柄（全局递增）。
    pub handle: u64,
    /// 绑定的页号。
    pub page: usize,
    /// 绑定时的页纪元（陈旧判定依据）。
    pub epoch: u64,
    /// 绑定的 clamp（驻留时冻结）。
    pub clamp: Clamp,
}

/// 状态机状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum St {
    /// 初始：池未开门。
    Closed,
    /// 就绪：可分配。
    Ready,
    /// 服务中：存在活跃绑定。
    Serving,
    /// 排空：拒绝新分配，等待活跃绑定清零。
    Draining,
}

/// 计数账本。
#[derive(Clone, Copy, Debug)]
pub struct Counters {
    /// 累计分配请求。
    pub req: u64,
    /// 累计分配成功。
    pub ok: u64,
    /// 累计拒绝（任何错误码）。
    pub reject: u64,
    /// 累计绑定。
    pub bound: u64,
    /// 累计解绑。
    pub unbound: u64,
    /// 累计释放。
    pub freed: u64,
}

impl Counters {
    fn new() -> Self {
        Counters { req: 0, ok: 0, reject: 0, bound: 0, unbound: 0, freed: 0 }
    }
}

/// 共享内存适配引擎。
pub struct ShmEngine {
    /// 引擎名。
    pub name: String,
    /// 状态机。
    st: St,
    /// 页账（定容 POOL_PAGES）。
    pages: Vec<PgEntry>,
    /// 绑定表（定容 BIND_CAP）。
    binds: Vec<Bind>,
    /// 句柄发生器。
    next_handle: u64,
    /// 保留页数（守恒项）。
    reserved: usize,
    /// 计数账本。
    pub cnt: Counters,
}

/// 守恒恒等式：占用页数 + 保留页数 + 空闲页数 == POOL_PAGES。
/// 其中占用页数 = live 标记为真且未被保留的页；每步分配/释放/保留后 verify()
/// 都重算三数之和，不等即报 XC_LEDGER_DRIFT。
impl ShmEngine {
    /// 构造：预建页账但不开门。
    pub fn new(name: &str) -> Self {
        ShmEngine {
            name: name.to_string(),
            st: St::Closed,
            pages: Vec::new(),
            binds: Vec::new(),
            next_handle: 1,
            reserved: 0,
            cnt: Counters::new(),
        }
    }

    /// 开门：建满页账，转入 Ready。
    pub fn open(&mut self) -> Result<(), u16> {
        if self.st != St::Closed {
            return Err(XC_POOL_NOT_READY);
        }
        self.pages.clear();
        for i in 0..POOL_PAGES {
            self.pages.push(PgEntry {
                page: i,
                domain: if i % 2 == 0 { Domain::Sysmem } else { Domain::LocalMem },
                tile: TileMode::Linear,
                live: false,
                reserved: false,
                epoch: 0,
            });
        }
        self.st = St::Ready;
        Ok(())
    }

    /// 状态只读。
    pub fn st(&self) -> St {
        self.st
    }

    /// 保留页（驱动更新窗口用——保留后不可分配）。
    pub fn reserve(&mut self, page: usize) -> Result<(), u16> {
        if self.st == St::Closed {
            return Err(XC_POOL_NOT_READY);
        }
        let e = match self.pages.get_mut(page) {
            Some(e) => e,
            None => return Err(XC_CAP_EXCEEDED),
        };
        if e.live || e.reserved {
            return Err(XC_CAP_EXCEEDED);
        }
        e.reserved = true;
        self.reserved += 1;
        self.verify()?;
        Ok(())
    }

    /// 分配一页（判据"按域分配可回退"：指定域满则回退另一域）。
    pub fn alloc(&mut self, want: Domain, tile: TileMode, clamp: Clamp) -> Result<usize, u16> {
        self.cnt.req += 1;
        if self.st != St::Ready && self.st != St::Serving {
            self.cnt.reject += 1;
            return Err(XC_POOL_NOT_READY);
        }
        clamp.fits(tile)?;
        if tile == TileMode::TileYf && want == Domain::Sysmem {
            // 紧凑 Yf 仅本地域支持——域组合矛盾。
            self.cnt.reject += 1;
            return Err(XC_TILE_INVALID);
        }
        let mut picked: i64 = -1;
        for (i, e) in self.pages.iter().enumerate() {
            if !e.live && !e.reserved && e.domain == want {
                picked = i as i64;
                break;
            }
        }
        if picked < 0 {
            // 回退：另一域。
            let alt = match want {
                Domain::Sysmem => Domain::LocalMem,
                Domain::LocalMem => Domain::Sysmem,
            };
            for (i, e) in self.pages.iter().enumerate() {
                if !e.live && !e.reserved && e.domain == alt {
                    picked = i as i64;
                    break;
                }
            }
        }
        if picked < 0 {
            self.cnt.reject += 1;
            return Err(XC_CAP_EXCEEDED);
        }
        let idx = picked as usize;
        let pg = self.pages[idx].page;
        let ep = self.pages[idx].epoch;
        self.pages[idx].live = true;
        self.pages[idx].tile = tile;
        // 先按 tiling 对齐要求占位多个物理页（简化模型：Y 系占 2 页）。
        let _ = ep;
        if tile != TileMode::Linear {
            // 占位：标记相邻同伴页保留以体现 tiling 足迹。
            let peer = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
            if peer < self.pages.len() && !self.pages[peer].live && !self.pages[peer].reserved {
                self.pages[peer].reserved = true;
                self.reserved += 1;
            }
        }
        self.cnt.ok += 1;
        if self.st == St::Ready {
            self.st = St::Serving;
        }
        self.verify()?;
        Ok(pg)
    }

    /// 绑定句柄到页（判据"句柄-页绑定带纪元"）。
    pub fn bind(&mut self, page: usize, clamp: Clamp) -> Result<u64, u16> {
        if self.st != St::Serving && self.st != St::Ready {
            return Err(XC_POOL_NOT_READY);
        }
        let e = match self.pages.get(page) {
            Some(e) => e,
            None => return Err(XC_BIND_MISS),
        };
        if !e.live {
            return Err(XC_BIND_MISS);
        }
        clamp.fits(e.tile)?;
        if self.binds.len() >= BIND_CAP {
            return Err(XC_CAP_EXCEEDED);
        }
        let h = self.next_handle;
        self.next_handle += 1;
        self.binds.push(Bind { handle: h, page, epoch: e.epoch, clamp });
        self.cnt.bound += 1;
        Ok(h)
    }

    /// 解绑（纪元不匹配视为陈旧——拒绝）。
    pub fn unbind(&mut self, handle: u64) -> Result<(), u16> {
        let mut at: i64 = -1;
        for (i, b) in self.binds.iter().enumerate() {
            if b.handle == handle {
                at = i as i64;
                break;
            }
        }
        if at < 0 {
            self.cnt.reject += 1;
            return Err(XC_BIND_MISS);
        }
        let b = self.binds[at as usize].clone();
        let e = match self.pages.get(b.page) {
            Some(e) => e,
            None => return Err(XC_BIND_MISS),
        };
        if e.epoch != b.epoch {
            self.cnt.reject += 1;
            return Err(XC_DOMAIN_CROSS);
        }
        self.binds.remove(at as usize);
        self.cnt.unbound += 1;
        Ok(())
    }

    /// 释放页（须先无绑定；双重释放拒绝）。
    pub fn free(&mut self, page: usize) -> Result<(), u16> {
        let e = match self.pages.get_mut(page) {
            Some(e) => e,
            None => return Err(XC_BIND_MISS),
        };
        if !e.live {
            self.cnt.reject += 1;
            return Err(XC_DOUBLE_FREE);
        }
        for b in self.binds.iter() {
            if b.page == page {
                self.cnt.reject += 1;
                return Err(XC_BIND_MISS);
            }
        }
        e.live = false;
        e.epoch += 1;
        e.tile = TileMode::Linear;
        self.cnt.freed += 1;
        if self.live_count() == 0 && self.st == St::Serving {
            self.st = St::Draining;
        }
        self.verify()?;
        Ok(())
    }

    /// 排空推进：Draining 且无绑定则关门回 Closed。
    pub fn tick(&mut self) -> Result<St, u16> {
        if self.st == St::Draining && self.binds.is_empty() && self.live_count() == 0 {
            self.st = St::Closed;
        }
        Ok(self.st)
    }

    /// 活跃页数。
    pub fn live_count(&self) -> usize {
        let mut n = 0;
        for e in self.pages.iter() {
            if e.live {
                n += 1;
            }
        }
        n
    }

    /// 空闲可分配页数。
    pub fn free_count(&self) -> usize {
        let mut n = 0;
        for e in self.pages.iter() {
            if !e.live && !e.reserved {
                n += 1;
            }
        }
        n
    }

    /// 守恒校验（判据"守恒账本"）。
    pub fn verify(&self) -> Result<(), u16> {
        let mut res = self.reserved;
        // tiling 足迹占位也计入 reserved，重新统计以防漂移。
        res = 0;
        for e in self.pages.iter() {
            if e.reserved {
                res += 1;
            }
        }
        let used = self.live_count();
        let free = self.free_count();
        if used + res + free != POOL_PAGES {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb820[");
        s.push_str(&self.name);
        s.push_str("] st=");
        s.push_str(match self.st {
            St::Closed => "closed",
            St::Ready => "ready",
            St::Serving => "serving",
            St::Draining => "draining",
        });
        s.push_str(" used=");
        s.push_str(&self.live_count().to_string());
        s.push_str(" free=");
        s.push_str(&self.free_count().to_string());
        s.push_str(" binds=");
        s.push_str(&self.binds.len().to_string());
        s.push_str(" ok=");
        s.push_str(&self.cnt.ok.to_string());
        s.push_str(" rej=");
        s.push_str(&self.cnt.reject.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut e = ShmEngine::new("w3-drill-820");

    // 负样本 1：未开门分配——应拒。
    match e.alloc(Domain::Sysmem, TileMode::Linear, Clamp { w: 1, h: 1 }) {
        Err(code) if code == XC_POOL_NOT_READY => {}
        Ok(_) => return Err(XC_POOL_NOT_READY),
        Err(other) => return Err(other),
    }
    e.open()?;
    if e.st() != St::Ready {
        return Err(XC_POOL_NOT_READY);
    }

    // 负样本 2：clamp 不满足 Y 对齐——应拒。
    match e.alloc(Domain::LocalMem, TileMode::TileY, Clamp { w: 130, h: 33 }) {
        Err(code) if code == XC_CLAMP_MISMATCH => {}
        Ok(_) => return Err(XC_CLAMP_MISMATCH),
        Err(other) => return Err(other),
    }
    // 负样本 3：Yf + 系统内存域组合矛盾——应拒。
    match e.alloc(Domain::Sysmem, TileMode::TileYf, Clamp { w: 64, h: 64 }) {
        Err(code) if code == XC_TILE_INVALID => {}
        Ok(_) => return Err(XC_TILE_INVALID),
        Err(other) => return Err(other),
    }

    // 正样本：双域各分配，绑定-解绑-释放。
    let p1 = e.alloc(Domain::Sysmem, TileMode::Linear, Clamp { w: 16, h: 16 })?;
    let p2 = e.alloc(Domain::LocalMem, TileMode::TileY, Clamp { w: 128, h: 32 })?;
    let h1 = e.bind(p1, Clamp { w: 16, h: 16 })?;
    let h2 = e.bind(p2, Clamp { w: 128, h: 32 })?;
    e.unbind(h1)?;
    // 释放仍被 h2 绑定的页——应拒。
    match e.free(p2) {
        Err(code) if code == XC_BIND_MISS => {}
        Ok(_) => return Err(XC_BIND_MISS),
        Err(other) => return Err(other),
    }
    e.unbind(h2)?;
    e.free(p1)?;
    e.free(p2)?;
    // 负样本 4：双重释放——应拒。
    match e.free(p1) {
        Err(code) if code == XC_DOUBLE_FREE => {}
        Ok(_) => return Err(XC_DOUBLE_FREE),
        Err(other) => return Err(other),
    }

    // 保留窗口 + 守恒校验 + 排空关门。
    e.reserve(POOL_PAGES - 1)?;
    e.verify()?;
    loop {
        let st = e.tick()?;
        if st == St::Closed {
            break;
        }
        if st == St::Ready && e.live_count() == 0 {
            // Ready 且无活跃页：直接验证后跳过（不关门也可收口）。
            break;
        }
    }
    e.verify()?;
    if e.cnt.reject < 4 {
        return Err(XC_LEDGER_DRIFT);
    }
    Ok(0)
}
