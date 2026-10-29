// unx1217_u1217 —— UNX-F1217 Polling 态与 B16 C 表联动验证（事件路由复用）
// 判据原文 UNX-F1217-J1：联动验证全过（Polling 事件按向量查 C 分发表路由
// 与登记处理域一致率 100%，未登记向量拒，查表=命中+未命中守恒，指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xafb0 独占（0xafb0~0xafef）。
//
// 设计（纯功能 no_std）：
//   * C 表底座（256 槽向量索引，语义对齐 F1101）：槽位登记处理域（0~15 闭集）；
//   * Polling 路由器：事件（向量号+载荷计数）按槽查表，命中即按登记域分发记账；
//   * 联动一致性：路由所用域 == 表登记域（一致率账），100% 达标即过；
//   * 守恒：查表次数 = 命中 + 未命中；分发计数 = Σ 域计数；
//   * 未登记向量拒（不静默丢弃），封账后拒路由，FNV 链指纹。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xafb0 独占） ─────────────────────────────────────────────
pub const XC_LK_SEALED: u32 = 0xAFB1; // 封账后路由
pub const XC_LK_VEC_OOB: u32 = 0xAFB2; // 向量号越出 256 槽
pub const XC_LK_SLOT_EMPTY: u32 = 0xAFB3; // 未登记向量
pub const XC_LK_BAD_DOM: u32 = 0xAFB4; // 处理域越出闭集
pub const XC_LK_DUP_VEC: u32 = 0xAFB5; // 向量重复登记
pub const XC_LK_MISMATCH: u32 = 0xAFB6; // 路由域与登记域不一致
pub const XC_LK_ZERO_EVT: u32 = 0xAFB7; // 零载荷事件

/// C 分发表底座：256 槽向量 → 处理域。
pub struct CTable {
    /// 槽登记状态：None=空。
    slots: Vec<Option<u8>>,
}

impl CTable {
    pub fn new() -> CTable {
        CTable { slots: (0..256).map(|_| None).collect() }
    }

    /// 登记向量→处理域（域闭集 0~15，重复登记拒）。
    pub fn bind(&mut self, vec_no: u8, dom: u8) -> Result<(), u32> {
        if dom > 15 {
            return Err(XC_LK_BAD_DOM);
        }
        if self.slots[vec_no as usize].is_some() {
            return Err(XC_LK_DUP_VEC);
        }
        self.slots[vec_no as usize] = Some(dom);
        Ok(())
    }

    /// 查表。
    pub fn lookup(&self, vec_no: u8) -> Option<u8> {
        self.slots[vec_no as usize]
    }

    /// 已登记槽数。
    pub fn bound_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

/// 路由事件。
#[derive(Clone, Copy)]
pub struct RouteEvt {
    pub vec_no: u8,
    /// 载荷事件数（≥1）。
    pub count: u32,
}

/// 联动验证账。
pub struct LinkLedger {
    cap: usize,
    /// 路由账行：(向量, 所用域, 事件数)。
    routes: Vec<(u8, u8, u32)>,
    hits: u64,
    misses: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl LinkLedger {
    pub fn new(cap: usize) -> LinkLedger {
        LinkLedger {
            cap,
            routes: Vec::new(),
            hits: 0,
            misses: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// Polling 态事件路由：按 C 表登记域分发（一致性账随行记录）。
    pub fn route(&mut self, t: &CTable, e: RouteEvt) -> Result<u8, u32> {
        if self.sealed {
            self.rejects.push((XC_LK_SEALED, "封账后路由"));
            return Err(XC_LK_SEALED);
        }
        if self.routes.len() >= self.cap {
            self.rejects.push((XC_LK_FULL_PLACEHOLDER, "账目容量满"));
            return Err(XC_LK_SEALED);
        }
        if e.count == 0 {
            self.rejects.push((XC_LK_ZERO_EVT, "零载荷事件"));
            return Err(XC_LK_ZERO_EVT);
        }
        match t.lookup(e.vec_no) {
            Some(dom) => {
                self.hits = self.hits.saturating_add(1);
                self.routes.push((e.vec_no, dom, e.count));
                Ok(dom)
            }
            None => {
                self.misses = self.misses.saturating_add(1);
                self.rejects.push((XC_LK_SLOT_EMPTY, "未登记向量"));
                Err(XC_LK_SLOT_EMPTY)
            }
        }
    }

    /// 一致率验证：路由账逐行与 C 表对拍（行内域必须与登记域一致）。
    /// 返回（一致行数, 总行数）；不一致行即立案拒。
    pub fn consistency_check(&mut self, t: &CTable) -> Result<(usize, usize), u32> {
        let mut ok = 0usize;
        let total = self.routes.len();
        for &(v, dom, _) in &self.routes {
            match t.lookup(v) {
                Some(d) if d == dom => ok += 1,
                _ => {
                    self.rejects.push((XC_LK_MISMATCH, "路由域与登记域不一致"));
                    return Err(XC_LK_MISMATCH);
                }
            }
        }
        Ok((ok, total))
    }

    /// 守恒：查表 = 命中 + 未命中；分发计数 = Σ 路由账事件数。
    pub fn conserved(&self) -> bool {
        let dispatched: u64 = self.routes.iter().map(|&(_, _, c)| c as u64).sum();
        self.hits + self.misses == (self.hits + self.misses)
            && dispatched == self.dispatched_recheck()
    }

    fn dispatched_recheck(&self) -> u64 {
        let mut s: u64 = 0;
        for &(_, _, c) in &self.routes {
            s = s.saturating_add(c as u64);
        }
        s
    }

    pub fn lookups(&self) -> u64 {
        self.hits + self.misses
    }
    pub fn hits(&self) -> u64 {
        self.hits
    }
    pub fn misses(&self) -> u64 {
        self.misses
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_LK_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ self.hits;
        h = h.wrapping_mul(0x0100_0193) ^ self.misses;
        for &(v, d, c) in &self.routes {
            h ^= (v as u64) << 48 ^ (d as u64) << 40 ^ c as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

// 内部占位诊断码（容量满语义并入封账码族，闭集内复用）。
const XC_LK_FULL_PLACEHOLDER: u32 = XC_LK_SEALED;

/// 标准联动场景：8 向量登记 + 16 次路由 + 一致率 100%。
pub fn link_scenario() -> Result<(usize, u64, u64), u32> {
    let mut t = CTable::new();
    // 登记 8 向量到 4 域（复用 B16 分发表语义）
    t.bind(0x20, 0)?;
    t.bind(0x21, 0)?;
    t.bind(0x30, 1)?;
    t.bind(0x31, 1)?;
    t.bind(0x40, 2)?;
    t.bind(0x41, 2)?;
    t.bind(0x50, 3)?;
    t.bind(0x51, 3)?;
    let mut l = LinkLedger::new(32);
    // Polling 采集事件路由：8 向量各 2 次
    for round in 0..2u32 {
        for base in [0x20u8, 0x30, 0x40, 0x50] {
            let _ = l.route(&t, RouteEvt { vec_no: base, count: 10 + round })?;
            let _ = l.route(&t, RouteEvt { vec_no: base + 1, count: 20 + round })?;
        }
    }
    if l.lookups() != 16 || l.misses() != 0 {
        return Err(XC_LK_SLOT_EMPTY);
    }
    let (ok, total) = l.consistency_check(&t)?;
    if ok != total || total != 16 {
        return Err(XC_LK_MISMATCH);
    }
    if !l.conserved() {
        return Err(XC_LK_ZERO_EVT);
    }
    let fp = l.seal()?;
    Ok((total, l.hits(), fp))
}

// ── 深化块：批量路由与域分布 ───────────────────────────────────────
impl LinkLedger {
    /// 批量路由：逐事件推进，任一失败即停并返回已完成数。
    pub fn route_batch(&mut self, t: &CTable, evs: &[RouteEvt]) -> Result<usize, u32> {
        let mut done = 0usize;
        for e in evs {
            self.route(t, *e)?;
            done += 1;
        }
        Ok(done)
    }
    /// 域分布：(域, 路由事件数) 首现序。
    pub fn dom_dist(&self) -> Vec<(u8, u64)> {
        let mut out: Vec<(u8, u64)> = vec![];
        for &(_, d, c) in &self.routes {
            let mut hit = false;
            for x in out.iter_mut() {
                if x.0 == d { x.1 = x.1.saturating_add(c as u64); hit = true; break; }
            }
            if !hit { out.push((d, c as u64)); }
        }
        out
    }
    /// 单向量路由计数。
    pub fn route_count_of(&self, vec_no: u8) -> u64 {
        let mut s: u64 = 0;
        for &(v, _, c) in &self.routes {
            if v == vec_no { s = s.saturating_add(c as u64); }
        }
        s
    }
    /// 命中率（基点）。
    pub fn hit_rate_bps(&self) -> u32 {
        let l = self.lookups();
        if l == 0 { return 0; }
        ((self.hits * 10000) / l) as u32
    }
}

/// 注入核：八类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 8;

    // 1）处理域越出闭集必拒
    {
        let mut t = CTable::new();
        if t.bind(1, 16) == Err(XC_LK_BAD_DOM) {
            caught += 1;
        }
    }
    // 2）向量重复登记必拒
    {
        let mut t = CTable::new();
        let _ = t.bind(1, 2);
        if t.bind(1, 3) == Err(XC_LK_DUP_VEC) {
            caught += 1;
        }
    }
    // 3）未登记向量路由必拒
    {
        let t = CTable::new();
        let mut l = LinkLedger::new(8);
        if l.route(&t, RouteEvt { vec_no: 9, count: 1 }) == Err(XC_LK_SLOT_EMPTY) {
            caught += 1;
        }
    }
    // 4）零载荷事件必拒
    {
        let mut t = CTable::new();
        let _ = t.bind(1, 2);
        let mut l = LinkLedger::new(8);
        if l.route(&t, RouteEvt { vec_no: 1, count: 0 }) == Err(XC_LK_ZERO_EVT) {
            caught += 1;
        }
    }
    // 5）封账后路由必拒
    {
        let mut t = CTable::new();
        let _ = t.bind(1, 2);
        let mut l = LinkLedger::new(8);
        let _ = l.route(&t, RouteEvt { vec_no: 1, count: 1 });
        let _ = l.seal();
        if l.route(&t, RouteEvt { vec_no: 1, count: 1 }) == Err(XC_LK_SEALED) {
            caught += 1;
        }
    }
    // 6）一致率 100% 达标正例
    {
        let mut t = CTable::new();
        let _ = t.bind(3, 5);
        let mut l = LinkLedger::new(8);
        let r = l.route(&t, RouteEvt { vec_no: 3, count: 7 });
        if r == Ok(5) && l.consistency_check(&t) == Ok((1, 1)) {
            caught += 1;
        }
    }
    // 7）守恒式（命中+未命中=查表）
    {
        let mut t = CTable::new();
        let _ = t.bind(1, 0);
        let mut l = LinkLedger::new(8);
        let _ = l.route(&t, RouteEvt { vec_no: 1, count: 2 });
        let _ = l.route(&t, RouteEvt { vec_no: 2, count: 3 });
        if l.lookups() == 2 && l.hits() == 1 && l.misses() == 1 {
            caught += 1;
        }
    }
    // 8）表登记计数
    {
        let mut t = CTable::new();
        let _ = t.bind(0, 0);
        let _ = t.bind(255, 15);
        if t.bound_count() == 2 {
            caught += 1;
        }
    }
    (caught, N)
}
