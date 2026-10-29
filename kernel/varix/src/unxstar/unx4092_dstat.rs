//! unx4092_dstat — UNX-F4092 dentry 统计账与命中率（诊断段 0x8DA0）。
//!
//! 判据原文：UNX-F4092-J1 命中/未命中/驱逐三账可导出，命中率复算一致
//!           （万次负载对照零差）。
//! 可验性质（本模块自证面）：
//!   J1 三账（hit / miss / evict）可一键导出为定长数组，且与分项计数器逐位相同；
//!   J2 万次负载后命中率整千分复算一致：rate_permille == hit * 1000 / lookup，
//!      独立复算路径与报告值逐位相等（零差）；
//!   J3 分账守恒：lookup == hit + miss；alloc - free == 在当前占用；
//!      evict <= free（驱逐是释放的一个子集，不得多于总释放）；
//!   J4 封账（Closed）后统计冻结：任何记账请求拒收（E_SEALED），导出仍可读；
//!   J5 非法记账拒收且不入账：未知事件码（E_EVENT_OOR）、空账出报告（E_EMPTY）、
//!      账不平出报告（E_BAL）——拒收路径不得改动任何计数器。
//! 设计纪律：
//!   1. 六槽定长数组 + 具名下标：导出就是拷数组，杜绝"导出时现算"导致的两套口径。
//!   2. 命中率只用整数千分率，不做浮点：hit * 1000 可能超 u32，用 u64 中间量再除
//!      （本模块整数算术，无浮点、无 panic）。
//!   3. "三账可导出"指 hit/miss/evict 三项独立可读，不被合成指标吃掉；合成指标
//!      （命中率）是派生量，必须能从三账复算出来——这就是 J2 的意义。
//!   4. 封账是单向的（Open → Closed），封账后 export 仍返回最后一次快照值，
//!      便于事后审计；解封不提供（要重开就新建账）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8DA0 本单独占。

pub const SEG_BASE: u16 = 0x8DA0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EVENT_OOR: u16 = SEG_BASE + 1; // 事件码越界
const E_SEALED: u16 = SEG_BASE + 2; // 已封账仍记账
const E_EMPTY: u16 = SEG_BASE + 3; // 空账出报告（零查找无命中率可言）
const E_BAL: u16 = SEG_BASE + 4; // 分账不平
const E_EVICT_OVER: u16 = SEG_BASE + 5; // 驱逐数超过释放数
const E_SLOT: u16 = SEG_BASE + 6; // 统计槽下标越界
const E_RATE: u16 = SEG_BASE + 7; // 命中率复算不一致
const E_OVER: u16 = SEG_BASE + 8; // 计数器触顶

/// 统计槽数（六槽：查找/命中/未命中/驱逐/建/释）。
pub const ST_N: usize = 6;

/// 槽位具名下标。
pub const ST_LOOKUP: usize = 0; // 查找总数
pub const ST_HIT: usize = 1;    // 命中
pub const ST_MISS: usize = 2;   // 未命中
pub const ST_EVICT: usize = 3;  // 驱逐（容量压力回收）
pub const ST_ALLOC: usize = 4;  // 新建条目
pub const ST_FREE: usize = 5;   // 释放条目（含驱逐）

/// 槽位名（导出/转储贴标签用）。
pub const ST_NAMES: [&str; ST_N] = ["lookup", "hit", "miss", "evict", "alloc", "free"];

/// 统计事件闭集：五种事件对应五种记法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatEvent {
    LookupHit = 0,  // 一次命中（lookup +1，hit +1）
    LookupMiss = 1, // 一次未命中（lookup +1，miss +1）
    Evict = 2,      // 一次驱逐（evict +1，free +1）
    Alloc = 3,      // 一次新建（alloc +1）
    Free = 4,       // 一次普通释放（free +1，不计驱逐）
}

/// 账相位：Open（可记）→ Closed（冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatPhase {
    Open = 0,   // 可记账
    Closed = 1, // 已封账，只读
}

/// 命中率报告：三账 + 两个派生率。
pub struct RateReport {
    pub lookup: u32,       // 查找总数
    pub hit: u32,          // 命中数
    pub miss: u32,         // 未命中数
    pub evict: u32,        // 驱逐数
    pub hit_permille: u32, // 命中率（千分）
    pub miss_permille: u32, // 未命中率（千分）
}

impl RateReport {
    /// J3 守恒：hit + miss == lookup。
    pub fn conserved(&self) -> bool {
        self.hit + self.miss == self.lookup
    }

    /// J2 独立复算：从三账重算千分率并与报告值比对。
    pub fn recompute_ok(&self) -> bool {
        if self.lookup == 0 {
            return self.hit_permille == 0 && self.miss_permille == 0;
        }
        let h = (self.hit as u64) * 1000u64 / (self.lookup as u64);
        let m = (self.miss as u64) * 1000u64 / (self.lookup as u64);
        h == self.hit_permille as u64 && m == self.miss_permille as u64
    }

    /// 两率之和：整数除法下允许差 1（舍入），故只判"≤ 1001 且 ≥ 999"。
    pub fn rates_sane(&self) -> bool {
        let s = self.hit_permille + self.miss_permille;
        s <= 1_001 && s >= 999
    }
}

/// 统计账本体。
pub struct DStatBook {
    pub v: [u32; ST_N],   // 六槽计数
    pub phase: StatPhase, // 相位
    pub events: u32,      // 成功记账次数
    pub rejects: u32,     // 拒收次数
    pub reports: u32,     // 出报告次数
}

impl DStatBook {
    pub fn new() -> DStatBook {
        DStatBook {
            v: [0u32; ST_N],
            phase: StatPhase::Open,
            events: 0,
            rejects: 0,
            reports: 0,
        }
    }

    /// 事件码 → 闭集。
    pub fn event_by_code(code: u16) -> Result<StatEvent, u16> {
        match code {
            0 => Ok(StatEvent::LookupHit),
            1 => Ok(StatEvent::LookupMiss),
            2 => Ok(StatEvent::Evict),
            3 => Ok(StatEvent::Alloc),
            4 => Ok(StatEvent::Free),
            _ => Err(E_EVENT_OOR),
        }
    }

    /// 记一笔事件：封账后全拒；计数触顶拒（不得回绕）。
    pub fn record(&mut self, ev: StatEvent) -> Result<(), u16> {
        if self.phase == StatPhase::Closed {
            self.rejects += 1;
            return Err(E_SEALED);
        }
        // 触顶预检：任一将被写入的槽达到 u32 上限即拒
        let bump = |v: u32| -> Result<(), u16> {
            if v == u32::MAX {
                Err(E_OVER)
            } else {
                Ok(())
            }
        };
        match ev {
            StatEvent::LookupHit => {
                bump(self.v[ST_LOOKUP])?;
                bump(self.v[ST_HIT])?;
                self.v[ST_LOOKUP] += 1;
                self.v[ST_HIT] += 1;
            }
            StatEvent::LookupMiss => {
                bump(self.v[ST_LOOKUP])?;
                bump(self.v[ST_MISS])?;
                self.v[ST_LOOKUP] += 1;
                self.v[ST_MISS] += 1;
            }
            StatEvent::Evict => {
                bump(self.v[ST_EVICT])?;
                bump(self.v[ST_FREE])?;
                self.v[ST_EVICT] += 1;
                self.v[ST_FREE] += 1;
            }
            StatEvent::Alloc => {
                bump(self.v[ST_ALLOC])?;
                self.v[ST_ALLOC] += 1;
            }
            StatEvent::Free => {
                bump(self.v[ST_FREE])?;
                self.v[ST_FREE] += 1;
            }
        }
        self.events += 1;
        Ok(())
    }

    /// 三账导出：(hit, miss, evict)。判据 J1 的"三账可导出"。
    pub fn export_three(&self) -> (u32, u32, u32) {
        (self.v[ST_HIT], self.v[ST_MISS], self.v[ST_EVICT])
    }

    /// 六槽全量导出（定长数组，逐位可核）。
    pub fn export(&self) -> [u32; ST_N] {
        let mut out = [0u32; ST_N];
        let mut i = 0usize;
        while i < ST_N {
            out[i] = self.v[i];
            i += 1;
        }
        out
    }

    /// 单槽读（越界拒）。
    pub fn slot(&self, idx: usize) -> Result<u32, u16> {
        if idx >= ST_N {
            return Err(E_SLOT);
        }
        Ok(self.v[idx])
    }

    /// 占用账：alloc - free（饱和到 0，且要求 alloc >= free）。
    pub fn in_use(&self) -> Result<u32, u16> {
        if self.v[ST_ALLOC] < self.v[ST_FREE] {
            return Err(E_BAL);
        }
        Ok(self.v[ST_ALLOC] - self.v[ST_FREE])
    }

    /// 出报告：空账拒、分账不平拒、驱逐超释放拒（J5）。
    pub fn report(&mut self) -> Result<RateReport, u16> {
        if self.v[ST_LOOKUP] == 0 {
            self.rejects += 1;
            return Err(E_EMPTY);
        }
        if self.v[ST_HIT] + self.v[ST_MISS] != self.v[ST_LOOKUP] {
            return Err(E_BAL);
        }
        if self.v[ST_EVICT] > self.v[ST_FREE] {
            return Err(E_EVICT_OVER);
        }
        if self.in_use().is_err() {
            return Err(E_BAL);
        }
        let lk = self.v[ST_LOOKUP] as u64;
        let h = (self.v[ST_HIT] as u64) * 1000u64 / lk;
        let m = (self.v[ST_MISS] as u64) * 1000u64 / lk;
        self.reports += 1;
        Ok(RateReport {
            lookup: self.v[ST_LOOKUP],
            hit: self.v[ST_HIT],
            miss: self.v[ST_MISS],
            evict: self.v[ST_EVICT],
            hit_permille: h as u32,
            miss_permille: m as u32,
        })
    }

    /// 封账：Open → Closed（单向）。
    pub fn close(&mut self) -> Result<(), u16> {
        if self.phase == StatPhase::Closed {
            return Err(E_SEALED);
        }
        self.phase = StatPhase::Closed;
        Ok(())
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EVENT_OOR => "事件码越界（合法 0..4）",
        E_SEALED => "已封账仍记账/重复封账",
        E_EMPTY => "空账出报告（零查找无命中率）",
        E_BAL => "分账不平（hit+miss != lookup 或 alloc < free）",
        E_EVICT_OVER => "驱逐数超过释放数",
        E_SLOT => "统计槽下标越界（合法 0..5）",
        E_RATE => "命中率复算不一致",
        E_OVER => "计数器触顶（不可再记）",
        _ => "未知诊断码",
    }
}

/// 确定性负载分类：按 i mod 20 定事件类型，保证万次负载的六槽计数可先验算出。
/// 0..11 命中(6000)、12..17 未命中(3000)、18 新建(500)、19 驱逐(500)。
/// 新建与驱逐同数，故结束占用恰为 0（判据 J3 的边界样本）。
fn classify(i: u64) -> StatEvent {
    let r = (i % 20) as u32;
    if r < 12 {
        StatEvent::LookupHit
    } else if r < 18 {
        StatEvent::LookupMiss
    } else if r == 18 {
        StatEvent::Alloc
    } else {
        StatEvent::Evict
    }
}

/// 确定性演练：万次负载 → 三账导出 → 复算零差 → 边界 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut bk = DStatBook::new();

    // R1：万次负载（命中 6,000 / 未命中 3,000 / 新建 500 / 驱逐 500）。
    let mut i = 0u64;
    while i < 10_000 {
        bk.record(classify(i))?;
        i += 1;
    }
    if bk.v[ST_LOOKUP] != 9_000 || bk.v[ST_HIT] != 6_000 || bk.v[ST_MISS] != 3_000 {
        return Err(E_BAL); // 先验算过的六槽计数必须逐位命中
    }
    if bk.v[ST_ALLOC] != 500 || bk.v[ST_EVICT] != 500 || bk.v[ST_FREE] != 500 {
        return Err(E_BAL);
    }
    if bk.events != 10_000 {
        return Err(E_BAL);
    }
    if bk.v[ST_LOOKUP] != bk.v[ST_HIT] + bk.v[ST_MISS] {
        return Err(E_BAL);
    }
    if bk.v[ST_LOOKUP] == 0 {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R2：三账可导出，且与分项槽逐位相同（J1）。
    let (h, m, e) = bk.export_three();
    if h != bk.v[ST_HIT] || m != bk.v[ST_MISS] || e != bk.v[ST_EVICT] {
        return Err(E_BAL);
    }
    let all = bk.export();
    let mut k = 0usize;
    while k < ST_N {
        if all[k] != bk.slot(k)? {
            return Err(E_BAL);
        }
        k += 1;
    }
    if all[ST_HIT] != h || all[ST_MISS] != m || all[ST_EVICT] != e {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：出报告并复算命中率——报告值与独立复算逐位相等（J2 主面）。
    let rep = bk.report()?;
    if !rep.conserved() {
        return Err(E_BAL);
    }
    if !rep.recompute_ok() {
        return Err(E_RATE);
    }
    if !rep.rates_sane() {
        return Err(E_RATE);
    }
    if rep.lookup != all[ST_LOOKUP] || rep.hit != all[ST_HIT] || rep.miss != all[ST_MISS] {
        return Err(E_BAL);
    }
    if rep.hit_permille != 666 || rep.miss_permille != 333 {
        return Err(E_RATE); // 6000/9000 = 666.6‰ → 666；3000/9000 = 333
    }
    // 手工复算一遍（第三套口径），必须与报告一致
    let manual = (bk.v[ST_HIT] as u64) * 1000u64 / (bk.v[ST_LOOKUP] as u64);
    if rep.hit_permille as u64 != manual {
        return Err(E_RATE);
    }
    hits += 1;

    // R4：占用账与驱逐子集关系——in_use == alloc - free（此处恰为 0），
    //     且 evict <= free（J3）。
    let in_use = bk.in_use()?;
    if in_use != 0 || in_use != bk.v[ST_ALLOC] - bk.v[ST_FREE] {
        return Err(E_BAL);
    }
    if bk.v[ST_EVICT] > bk.v[ST_FREE] {
        return Err(E_EVICT_OVER);
    }
    hits += 1;

    // R5：全命中/全未命中两个极端率——1000 / 0 与 0 / 1000（边界千分率）。
    let mut a = DStatBook::new();
    let mut j = 0u32;
    while j < 50 {
        a.record(StatEvent::LookupHit)?;
        j += 1;
    }
    let ra = a.report()?;
    if ra.hit_permille != 1000 || ra.miss_permille != 0 {
        return Err(E_RATE);
    }
    let mut b = DStatBook::new();
    j = 0;
    while j < 50 {
        b.record(StatEvent::LookupMiss)?;
        j += 1;
    }
    let rb = b.report()?;
    if rb.hit_permille != 0 || rb.miss_permille != 1000 {
        return Err(E_RATE);
    }
    hits += 1;

    // R6：封账——记账全拒（E_SEALED），导出与报告仍可读，且值不再变（J4）。
    bk.close()?;
    let snapshot = bk.export();
    let rej0 = bk.rejects;
    if bk.record(StatEvent::LookupHit) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    if bk.record(StatEvent::Evict) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    if bk.rejects != rej0 + 2 {
        return Err(E_BAL);
    }
    let snap2 = bk.export();
    let mut q = 0usize;
    while q < ST_N {
        if snapshot[q] != snap2[q] {
            return Err(E_BAL); // 封账后账不得暗改
        }
        q += 1;
    }
    if bk.close() != Err(E_SEALED) {
        return Err(E_SEALED); // 重复封账拒
    }
    hits += 1;

    // I1：事件码越界（5 / 65535）→ E_EVENT_OOR，不入账。
    if DStatBook::event_by_code(5) != Err(E_EVENT_OOR) {
        return Err(E_EVENT_OOR);
    }
    if DStatBook::event_by_code(0xFFFF) != Err(E_EVENT_OOR) {
        return Err(E_EVENT_OOR);
    }
    hits += 1;

    // I2：空账出报告 → E_EMPTY（零查找没有命中率）。
    let mut empty = DStatBook::new();
    match empty.report() {
        Err(E_EMPTY) => {}
        _ => return Err(E_EMPTY),
    }
    hits += 1;

    // I3：分账不平注入——直接把 hit 改大，报告必须拒（E_BAL）。
    let mut bad = DStatBook::new();
    bad.record(StatEvent::LookupMiss)?;
    bad.v[ST_HIT] = 5; // 人为破坏守恒
    match bad.report() {
        Err(E_BAL) => {}
        _ => return Err(E_BAL),
    }
    hits += 1;

    // I4：驱逐超释放注入——evict 大于 free 必须拒（E_EVICT_OVER）。
    let mut bad2 = DStatBook::new();
    bad2.record(StatEvent::LookupHit)?;
    bad2.v[ST_EVICT] = 3;
    bad2.v[ST_FREE] = 1;
    match bad2.report() {
        Err(E_EVICT_OVER) => {}
        _ => return Err(E_EVICT_OVER),
    }
    hits += 1;

    // I5：槽下标越界（6 / 65535）→ E_SLOT；alloc < free 时 in_use 拒（E_BAL）。
    if bk.slot(ST_N) != Err(E_SLOT) {
        return Err(E_SLOT);
    }
    if bk.slot(0xFFFF) != Err(E_SLOT) {
        return Err(E_SLOT);
    }
    let mut bad3 = DStatBook::new();
    bad3.v[ST_ALLOC] = 0;
    bad3.v[ST_FREE] = 2;
    if bad3.in_use().is_err() == false {
        return Err(E_BAL);
    }
    hits += 1;

    // 终审：封账前那本账三账仍然自洽（导出/报告对同一份数据给出同一结论）。
    let fin = bk.report()?;
    if !fin.conserved() || !fin.recompute_ok() {
        return Err(E_RATE);
    }
    if fin.evict != snapshot[ST_EVICT] {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
