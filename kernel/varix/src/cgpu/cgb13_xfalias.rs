//! cgb13_xfalias — CGPU-F0226 历史缓冲别名分析。
//!
//! 判据原文（施工书锚点 CGPU-F0226）：别名分析：ping-pong/环形池的跨帧别名
//! 关系推导（帧 N 写目标=帧 N-1 读源的同一物理缓冲判定）；别名图构建与传播
//! （资源重命名/池回收后的别名边维护）；误别名防护（误判别名→数据竞争——
//! 保守策略：不确定即视为别名）；与 B02 依赖解析联动（帧内依赖+跨帧别名=
//! 完整依赖视图）。判据：别名推导、传播、保守策略、联动。
//!
//! 职责定位：逻辑资源名（帧内视角）到物理缓冲（跨帧视角）的别名账本。环形
//! 池里"帧 N 的写目标"与"帧 N-1 的读源"常常是同一块物理内存——这条别名边
//! 不进依赖视图，写后读一致性就是空话。三类推导：①ping-pong 双缓冲推导
//! （奇偶帧互换写读位——位次相同即别名）；②环形池槽位推导（(frame mod
//! slots) 同槽即别名）；③重命名传播（资源改名/池回收后别名边跟随物理身份
//! 走，不跟名字走）。误别名防护是本模块的存在理由：两资源是否同物理缓冲，
//! 只有显式登记的池布局可以证明；证明不了就按别名处理——误判别名的代价是
//! 数据竞争，误保守的代价只是一条多余同步。诊断码段 0xD420。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_ALIAS_VERSION: &str = "B13-xfalias-v1";

// ---------------------------------------------------------------------------
// 诊断码（0xD420 槽，本会话独占）
// ---------------------------------------------------------------------------

/// 池布局未登记就推导别名（没有物理布局证明，拒绝臆测推导——但推导面仍走保守）。
pub const XC_LAYOUT_UNKNOWN: u32 = 0xD420;
/// 物理缓冲重复绑定（两个逻辑资源绑到同一物理槽且声明独占——账目矛盾）。
pub const XC_PHYS_DUP_EXCLUSIVE: u32 = 0xD421;
/// 环形池槽位数为零（布局非法）。
pub const XC_RING_ZERO_SLOTS: u32 = 0xD422;
/// 别名查询目标不在册。
pub const XC_ALIAS_TARGET_MISSING: u32 = 0xD423;
/// 独占声明与别名事实冲突（声明独占但别名图说共享——以别名图为真，声明拒收）。
pub const XC_EXCLUSIVE_CONFLICT: u32 = 0xD424;
/// 布局代际倒退（池重登记时槽位数变小且仍有在册别名边——会撕裂已有推导）。
pub const XC_LAYOUT_SHRINK: u32 = 0xD425;
/// 别名边账容量满。
pub const XC_ALIAS_FULL: u32 = 0xD426;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 别名边账容量。
pub const ALIAS_EDGE_CAP: usize = 2048;
/// 物理池登记上限。
pub const POOL_CAP: usize = 64;
/// 保守判定口径声明（写死并注释来源：施工书"不确定即视为别名"）。
pub const CONSERVATIVE_ON_UNKNOWN: bool = true;

// ---------------------------------------------------------------------------
// 基础类型
// ---------------------------------------------------------------------------

/// 逻辑资源：帧内视角的名字（生产者起的名，随帧走）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogicalRes {
    /// 所属帧号。
    pub frame: u64,
    /// 帧内资源序号。
    pub slot: u32,
}

impl LogicalRes {
    pub fn key(&self) -> String {
        let mut s = String::from("f");
        s.push_str(&itoa(self.frame));
        s.push_str("s");
        s.push_str(&itoa(self.slot as u64));
        s
    }
}

/// 物理池布局：环形池的槽位语义真源。
#[derive(Clone, Debug)]
pub struct RingPool {
    /// 池名。
    pub name: String,
    /// 槽位数。
    pub slots: u32,
    /// 布局代际（每次重登记 +1——推导都带代际戳，跨代际不混算）。
    pub generation: u64,
    /// 槽位 → 当前绑定的物理缓冲号。
    pub slot_phys: Vec<u64>,
    /// 本池是否声明独占（独占池的槽位不允许别名共享声明）。
    pub exclusive: bool,
}

impl RingPool {
    /// 帧号到槽位的映射（环形池推导核心：frame mod slots）。
    pub fn slot_of(&self, frame: u64) -> u32 {
        if self.slots == 0 {
            return 0;
        }
        (frame % self.slots as u64) as u32
    }
}

/// 一条别名判定记录。
#[derive(Clone, Debug)]
pub struct AliasEdge {
    /// 推导代际。
    pub generation: u64,
    /// 逻辑资源 A。
    pub a: LogicalRes,
    /// 逻辑资源 B。
    pub b: LogicalRes,
    /// 共同物理缓冲号（Unknown 时为 None）。
    pub phys: Option<u64>,
    /// 判定来源。
    pub via: AliasVia,
    /// 是否保守判定（无法证明同物理、按别名处理）。
    pub conservative: bool,
}

/// 别名判定来源闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AliasVia {
    /// ping-pong 位次推导。
    PingPong,
    /// 环形池同槽推导。
    RingSlot,
    /// 重命名传播继承。
    RenameProp,
    /// 保守兜底（无证明，按别名处理）。
    Conservative,
}

impl AliasVia {
    pub const fn zh(self) -> &'static str {
        match self {
            AliasVia::PingPong => "乒乓位次",
            AliasVia::RingSlot => "环槽同位",
            AliasVia::RenameProp => "改名传播",
            AliasVia::Conservative => "保守兜底",
        }
    }
}

// ---------------------------------------------------------------------------
// 别名分析引擎
// ---------------------------------------------------------------------------

/// 别名分析引擎：逻辑名 ↔ 物理缓冲 的判定正本。
pub struct AliasEngine {
    pub name: String,
    /// 在册物理池。
    pools: Vec<RingPool>,
    /// 别名边账。
    edges: Vec<AliasEdge>,
    /// 逻辑资源 → 物理缓冲的显式绑定（证明来源；未绑定 = 不确定）。
    bindings: Vec<(LogicalRes, u64)>,
    /// 逻辑资源声明独占账。
    exclusives: Vec<LogicalRes>,
    /// 拒绝账。
    rejections: Vec<(u32, String)>,
    /// 统计。
    pub total_derived: u64,
    pub total_conservative: u64,
    pub total_rejects: u64,
}

impl AliasEngine {
    pub fn new(name: &str) -> Self {
        AliasEngine {
            name: String::from(name),
            pools: Vec::new(),
            edges: Vec::new(),
            bindings: Vec::new(),
            exclusives: Vec::new(),
            rejections: Vec::new(),
            total_derived: 0,
            total_conservative: 0,
            total_rejects: 0,
        }
    }

    fn reject(&mut self, code: u32, why: &str) -> u32 {
        self.rejections.push((code, String::from(why)));
        if self.rejections.len() > 256 {
            self.rejections.remove(0);
        }
        self.total_rejects += 1;
        code
    }

    // ---------------- 池登记 ----------------

    /// 登记环形池布局（首次或重登记）。重登记槽位收窄且仍有别名边 → 拒（会
    /// 撕裂已有推导——布局代际只进不退）。
    pub fn register_pool(&mut self, name: &str, slots: u32, exclusive: bool) -> Result<u64, u32> {
        if slots == 0 {
            return Err(self.reject(XC_RING_ZERO_SLOTS, "环形池槽位数为零"));
        }
        let mut gen = 1;
        if let Some(p) = self.pools.iter_mut().find(|p| p.name == name) {
            if slots < p.slots && !self.edges.is_empty() {
                return Err(self.reject(XC_LAYOUT_SHRINK, "槽位收窄且别名边在册"));
            }
            gen = p.generation + 1;
            p.slots = slots;
            p.generation = gen;
            p.exclusive = exclusive;
            p.slot_phys.clear();
        } else {
            if self.pools.len() >= POOL_CAP {
                return Err(self.reject(XC_ALIAS_FULL, "物理池登记上限"));
            }
            let mut slot_phys = Vec::new();
            let mut i = 0;
            while i < slots {
                slot_phys.push(0);
                i += 1;
            }
            self.pools.push(RingPool {
                name: String::from(name),
                slots,
                generation: gen,
                slot_phys,
                exclusive,
            });
        }
        Ok(gen)
    }

    fn pool_of(&self, name: &str) -> Option<&RingPool> {
        self.pools.iter().find(|p| p.name == name)
    }

    fn pool_of_mut(&mut self, name: &str) -> Option<&mut RingPool> {
        self.pools.iter_mut().find(|p| p.name == name)
    }

    /// 逻辑资源绑定物理缓冲（显式证明来源）。
    pub fn bind(&mut self, res: LogicalRes, phys: u64) -> Result<(), u32> {
        if self.bindings.iter().any(|(r, p)| r == &res && *p != phys) {
            return Err(self.reject(XC_PHYS_DUP_EXCLUSIVE, "逻辑资源重复绑定不同物理缓冲"));
        }
        // 独占声明核对：同物理缓冲的第二个独占绑定冲突。
        if self.exclusives.iter().any(|x| {
            self.bindings
                .iter()
                .any(|(r, p)| r == x && *p == phys && *r != res)
        }) {
            return Err(self.reject(XC_EXCLUSIVE_CONFLICT, "独占声明与既有绑定冲突"));
        }
        if !self.bindings.iter().any(|(r, _)| r == &res) {
            self.bindings.push((res, phys));
        }
        Ok(())
    }

    /// 声明逻辑资源独占。
    pub fn declare_exclusive(&mut self, res: LogicalRes) -> Result<(), u32> {
        let phys = match self.bindings.iter().find(|(r, _)| r == &res) {
            Some((_, p)) => *p,
            None => return Err(self.reject(XC_ALIAS_TARGET_MISSING, "独占声明前须先绑定")),
        };
        if self.bindings.iter().any(|(r, p)| *p == phys && *r != res) {
            return Err(self.reject(XC_EXCLUSIVE_CONFLICT, "独占声明与别名事实冲突"));
        }
        if !self.exclusives.contains(&res) {
            self.exclusives.push(res);
        }
        Ok(())
    }

    // ---------------- 推导 ----------------

    /// ping-pong 推导：双缓冲池在帧 f 与 f-1 上的写读位次相同 → 别名。
    /// 返回推导出的别名边数。
    pub fn derive_pingpong(&mut self, pool: &str, frame: u64) -> Result<usize, u32> {
        let (gen, slots) = match self.pool_of(pool) {
            Some(p) => (p.generation, p.slots),
            None => return Err(self.reject(XC_LAYOUT_UNKNOWN, "乒乓推导池未登记")),
        };
        if slots < 2 {
            return Err(self.reject(XC_RING_ZERO_SLOTS, "乒乓池至少双槽"));
        }
        let mut added = 0;
        let mut s = 0;
        while s < slots {
            let a = LogicalRes { frame, slot: s };
            let b = LogicalRes { frame: frame - 1, slot: s };
            let phys = self.pool_of(pool).and_then(|p| p.slot_phys.get(s as usize).copied());
            added += self.push_alias(gen, a, b, phys, AliasVia::PingPong, false)?;
            s += 1;
        }
        Ok(added)
    }

    /// 环形池推导：帧 f 与 f-k（k < slots）同槽 → 别名。
    pub fn derive_ring(&mut self, pool: &str, frame: u64, back: u64) -> Result<usize, u32> {
        let (gen, slots, slot, phys) = match self.pool_of(pool) {
            Some(p) => (
                p.generation,
                p.slots,
                p.slot_of(frame),
                p.slot_phys.get(p.slot_of(frame) as usize).copied(),
            ),
            None => return Err(self.reject(XC_LAYOUT_UNKNOWN, "环槽推导池未登记")),
        };
        if back == 0 || back >= slots as u64 {
            return Err(self.reject(XC_RING_ZERO_SLOTS, "回溯步数非法（0 或 ≥ 槽数）"));
        }
        if frame < back {
            return Err(self.reject(XC_RING_ZERO_SLOTS, "帧号不足以回溯"));
        }
        let a = LogicalRes { frame, slot };
        let b = LogicalRes { frame: frame - back, slot };
        self.push_alias(gen, a, b, phys, AliasVia::RingSlot, false)
    }

    /// 保守兜底：无法证明同物理也按别名处理（误别名→数据竞争，宁多同步）。
    pub fn derive_conservative(&mut self, a: LogicalRes, b: LogicalRes) -> Result<usize, u32> {
        self.total_conservative += 1;
        self.push_alias(0, a, b, None, AliasVia::Conservative, true)
    }

    /// 重命名传播：逻辑资源改名（帧递进）后，旧别名边映射到新名。
    /// 物理身份不变——别名边跟随 phys 走。
    pub fn propagate_rename(
        &mut self,
        old: LogicalRes,
        new: LogicalRes,
        phys: u64,
    ) -> Result<usize, u32> {
        self.bind(new, phys)?;
        let gen = 0; // 传播不带池代际——它继承的是物理身份证明。
        let mut added = 0;
        let n = self.edges.len();
        let mut i = 0;
        while i < n {
            let e = &self.edges[i];
            if e.a == old || e.b == old {
                let other = if e.a == old { e.b.clone() } else { e.a.clone() };
                let (a2, b2) = if e.a == old { (new.clone(), other) } else { (other, new.clone()) };
                added += self.push_alias(gen, a2, b2, Some(phys), AliasVia::RenameProp, false)?;
            }
            i += 1;
        }
        Ok(added)
    }

    fn push_alias(
        &mut self,
        generation: u64,
        a: LogicalRes,
        b: LogicalRes,
        phys: Option<u64>,
        via: AliasVia,
        conservative: bool,
    ) -> Result<usize, u32> {
        if a == b {
            return Ok(0);
        }
        // 去重：双向同对同来源不重记。
        let dup = self.edges.iter().any(|e| {
            (e.a == a && e.b == b || e.a == b && e.b == a) && e.via == via
        });
        if dup {
            return Ok(0);
        }
        if self.edges.len() >= ALIAS_EDGE_CAP {
            return Err(self.reject(XC_ALIAS_FULL, "别名边账容量满"));
        }
        self.edges.push(AliasEdge { generation, a, b, phys, via, conservative });
        self.total_derived += 1;
        Ok(1)
    }

    // ---------------- 查询与核验 ----------------

    /// 两逻辑资源是否别名（含保守判定：不在册不确定 → 按 CONSERVATIVE_ON_UNKNOWN）。
    pub fn is_alias(&self, a: &LogicalRes, b: &LogicalRes) -> bool {
        if a == b {
            return true;
        }
        let proven_same = match (self.phys_of(a), self.phys_of(b)) {
            (Some(pa), Some(pb)) => pa == pb,
            _ => CONSERVATIVE_ON_UNKNOWN,
        };
        if proven_same {
            return true;
        }
        self.edges
            .iter()
            .any(|e| (e.a == *a && e.b == *b) || (e.a == *b && e.b == *a))
    }

    /// 逻辑资源的物理绑定。
    pub fn phys_of(&self, res: &LogicalRes) -> Option<u64> {
        self.bindings.iter().find(|(r, _)| r == res).map(|(_, p)| *p)
    }

    /// 与 B02 依赖解析联动的完整视图行：帧内依赖（B02 管）+ 跨帧别名（本模块
    /// 管）——本函数产出别名侧的等待清单（消费方在写别名资源前须等待的帧）。
    pub fn dependency_view_rows(&self, frame: u64) -> Vec<String> {
        let mut rows = Vec::new();
        for e in self.edges.iter() {
            if e.b.frame == frame && e.a.frame < frame {
                let mut s = String::from("等待 f");
                s.push_str(&itoa(e.a.frame));
                s.push_str("s");
                s.push_str(&itoa(e.a.slot as u64));
                s.push_str(" 别名来源=");
                s.push_str(e.via.zh());
                if e.conservative {
                    s.push_str("（保守）");
                }
                rows.push(s);
            }
        }
        rows
    }

    /// 别名账守恒：每条 RenameProp 边必须能找到同物理的祖先边或绑定证明。
    pub fn validate(&self) -> Result<(), u32> {
        for e in self.edges.iter() {
            if e.via == AliasVia::RenameProp {
                let proven = self.phys_of(&e.a).is_some() || self.phys_of(&e.b).is_some();
                if !proven {
                    return Err(XC_ALIAS_TARGET_MISSING);
                }
            }
        }
        Ok(())
    }

    /// 快照。
    pub fn snapshot(&self) -> AliasSnap {
        AliasSnap {
            version: String::from(XF_ALIAS_VERSION),
            engine: self.name.clone(),
            pools: self.pools.len() as u64,
            bindings: self.bindings.len() as u64,
            edges: self.edges.len() as u64,
            conservative: self.total_conservative,
            derived: self.total_derived,
            rejects: self.total_rejects,
        }
    }
}

/// 别名快照。
#[derive(Clone, Debug)]
pub struct AliasSnap {
    pub version: String,
    pub engine: String,
    pub pools: u64,
    pub bindings: u64,
    pub edges: u64,
    pub conservative: u64,
    pub derived: u64,
    pub rejects: u64,
}

impl AliasSnap {
    pub fn detail(&self) -> String {
        let mut s = String::from("[别名 ");
        s.push_str(&self.engine);
        s.push_str(" 池=");
        s.push_str(&itoa(self.pools));
        s.push_str(" 绑定=");
        s.push_str(&itoa(self.bindings));
        s.push_str(" 边=");
        s.push_str(&itoa(self.edges));
        s.push_str(" 推导=");
        s.push_str(&itoa(self.derived));
        s.push_str(" 保守=");
        s.push_str(&itoa(self.conservative));
        s.push_str(" 拒绝=");
        s.push_str(&itoa(self.rejects));
        s.push_str("]");
        s
    }
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    match core::str::from_utf8(&buf[i..]) {
        Ok(s) => String::from(s),
        Err(_) => String::from("?"),
    }
}

// ---------------------------------------------------------------------------
// 场景化构造（纯函数，确定性）
// ---------------------------------------------------------------------------

/// 场景 A：四槽环形池回溯推导——f8 与 f5、f4 与 f1 同槽。
pub fn scenario_ring_alias() -> Result<AliasSnap, u32> {
    let mut e = AliasEngine::new("B13-环池");
    e.register_pool("历史色板", 4, false)?;
    // 帧号布局绑定：f1..f8 的写槽位绑定物理缓冲 100+槽。
    let mut f = 1;
    while f <= 8 {
        let slot = (f % 4) as u32;
        e.bind(LogicalRes { frame: f, slot }, 100 + slot as u64)?;
        f += 1;
    }
    e.derive_ring("历史色板", 8, 3)?;
    e.derive_ring("历史色板", 4, 3)?;
    if !e.is_alias(&LogicalRes { frame: 8, slot: 0 }, &LogicalRes { frame: 4, slot: 0 }) {
        return Err(XC_ALIAS_TARGET_MISSING);
    }
    e.validate()?;
    Ok(e.snapshot())
}

/// 场景 B：保守兜底——未绑定资源对按别名处理（误别名防线）。
pub fn scenario_conservative() -> Result<AliasSnap, u32> {
    let mut e = AliasEngine::new("B13-保守");
    let a = LogicalRes { frame: 10, slot: 1 };
    let b = LogicalRes { frame: 9, slot: 1 };
    if e.is_alias(&a, &b) != CONSERVATIVE_ON_UNKNOWN {
        return Err(XC_LAYOUT_UNKNOWN);
    }
    e.derive_conservative(a, b)?;
    e.validate()?;
    Ok(e.snapshot())
}

/// 场景 C：独占冲突负向——同物理两个独占声明必须被拒。
pub fn scenario_exclusive_conflict() -> Result<(), u32> {
    let mut e = AliasEngine::new("B13-独占拒");
    let a = LogicalRes { frame: 1, slot: 0 };
    let b = LogicalRes { frame: 2, slot: 0 };
    e.bind(a, 500)?;
    e.bind(b, 500)?;
    e.declare_exclusive(a)?;
    match e.declare_exclusive(b) {
        Err(code) if code == XC_EXCLUSIVE_CONFLICT => Ok(()),
        Ok(()) => Err(XC_EXCLUSIVE_CONFLICT),
        Err(other) => Err(other),
    }
}

/// 场景 D：改名传播——物理身份跟随，别名边映射到新名。
pub fn scenario_rename_propagate() -> Result<AliasSnap, u32> {
    let mut e = AliasEngine::new("B13-改名");
    let old = LogicalRes { frame: 3, slot: 2 };
    let new = LogicalRes { frame: 7, slot: 2 };
    e.bind(old, 900)?;
    e.derive_conservative(old, LogicalRes { frame: 2, slot: 2 })?;
    let added = e.propagate_rename(old, new, 900)?;
    if added == 0 {
        return Err(XC_ALIAS_TARGET_MISSING);
    }
    e.validate()?;
    Ok(e.snapshot())
}
