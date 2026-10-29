//! CGPU-F0728 · 场景内容遥测（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0728，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：场景内容遥测——场景内容驻留账（加载/卸载/命中/未命中/
//!    逐出）五类事件计数、驻留守恒、命中率千分比与热点 TopK。
//!
//! 设计纪律：
//! - 场景账定容：SCENE_CAP=32 个场景槽，按场景号建卡。
// - 驻留守恒恒等式：registered == resident + evicted（逐场景显式）。
// - 事件五类闭集：Load/Unload/Hit/Miss/Evict，类计数器结构体。
// - 命中率：hits*1000/(hits+misses)（无访问时返回 Err）。
// - 热点 TopK：按访问量（hits+misses）降序取 K（K ≤ 槽数），
//   同量按场景号升序（全确定性）。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4940 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4940，批内 CGPU-F0728 独占）。
pub const SEG_BASE: u16 = 0x4940;

/// 场景账已满。
pub const E_FULL: u16 = SEG_BASE + 1;
/// 未登记场景被引用。
pub const E_UNKNOWN: u16 = SEG_BASE + 2;
/// 状态迁移非法（重复加载/卸载未加载）。
pub const E_STATE: u16 = SEG_BASE + 3;
/// 驻留守恒被击穿。
pub const E_LEDGER: u16 = SEG_BASE + 4;
/// 无访问样本时算命中率。
pub const E_NO_ACCESS: u16 = SEG_BASE + 5;
/// TopK 参数非法（0 或超槽容）。
pub const E_K: u16 = SEG_BASE + 6;

/// 场景槽容量。
pub const SCENE_CAP: usize = 32;

/// 场景内容事件（闭集五类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneEvt {
    /// 内容加载入驻留。
    Load,
    /// 主动卸载。
    Unload,
    /// 驻留命中。
    Hit,
    /// 未命中（触发加载）。
    Miss,
    /// 容量逐出。
    Evict,
}

/// 五类计数器。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvtCounters {
    pub load: u64,
    pub unload: u64,
    pub hit: u64,
    pub miss: u64,
    pub evict: u64,
}

impl EvtCounters {
    fn bump(&mut self, e: SceneEvt) {
        match e {
            SceneEvt::Load => self.load += 1,
            SceneEvt::Unload => self.unload += 1,
            SceneEvt::Hit => self.hit += 1,
            SceneEvt::Miss => self.miss += 1,
            SceneEvt::Evict => self.evict += 1,
        }
    }
    /// 访问量。
    fn accesses(&self) -> u64 {
        self.hit + self.miss
    }
}

/// 场景卡：单场景内容账。
///
/// 驻留守恒（显式恒等式）：
///   registered == resident + evicted
///   其中 registered = load 计数，evicted = evict 计数，
///   resident = 当前驻留标志推导值（load 未卸未逐）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneCard {
    /// 场景号。
    pub id: u16,
    /// 当前是否驻留。
    pub resident: bool,
    /// 五类计数。
    pub c: EvtCounters,
    /// 累计逐出。
    pub evicted: u64,
}

impl SceneCard {
    /// 守恒：load == (resident as u64 折算当前驻留) 需谨慎——
    /// 采用会计口径：load_count - unload_count - evict_count == resident(0/1)。
    fn conserved(&self) -> bool {
        let live = self.c.load.saturating_sub(self.c.unload).saturating_sub(self.c.evict);
        live == self.resident as u64
    }
}

/// 场景内容遥测器。
#[derive(Clone, Debug)]
pub struct SceneTelemetry {
    cards: Vec<SceneCard>,
    /// 全局事件计数（跨场景）。
    total: EvtCounters,
}

impl SceneTelemetry {
    pub fn new() -> Self {
        Self {
            cards: Vec::new(),
            total: EvtCounters::default(),
        }
    }
    /// 登记场景（未驻留，计数清零）。
    pub fn register(&mut self, id: u16) -> Result<(), u16> {
        if self.cards.len() >= SCENE_CAP {
            return Err(E_FULL);
        }
        if self.cards.iter().any(|c| c.id == id) {
            return Err(E_STATE);
        }
        self.cards.push(SceneCard {
            id,
            resident: false,
            c: EvtCounters::default(),
            evicted: 0,
        });
        Ok(())
    }
    /// 取可变卡。
    fn card_mut(&mut self, id: u16) -> Result<&mut SceneCard, u16> {
        match self.cards.iter_mut().find(|c| c.id == id) {
            Some(c) => Ok(c),
            None => Err(E_UNKNOWN),
        }
    }
    /// 记一条事件（自动维护驻留态与守恒）。
    pub fn observe(&mut self, id: u16, e: SceneEvt) -> Result<(), u16> {
        {
            let c = self.card_mut(id)?;
            match e {
                SceneEvt::Load => {
                    if c.resident {
                        return Err(E_STATE);
                    }
                    c.resident = true;
                }
                SceneEvt::Unload => {
                    if !c.resident {
                        return Err(E_STATE);
                    }
                    c.resident = false;
                }
                SceneEvt::Evict => {
                    if !c.resident {
                        return Err(E_STATE);
                    }
                    c.resident = false;
                    c.evicted += 1;
                }
                SceneEvt::Hit | SceneEvt::Miss => {
                    if !c.resident && e == SceneEvt::Hit {
                        return Err(E_STATE); // 未驻留不可能命中
                    }
                }
            }
            c.c.bump(e);
        }
        self.total.bump(e);
        Ok(())
    }
    /// 全账守恒审计。
    pub fn audit(&self) -> Result<(), u16> {
        for c in &self.cards {
            if !c.conserved() {
                return Err(E_LEDGER);
            }
        }
        // 全局与逐卡和一致。
        let sum = EvtCounters {
            load: self.cards.iter().map(|c| c.c.load).sum(),
            unload: self.cards.iter().map(|c| c.c.unload).sum(),
            hit: self.cards.iter().map(|c| c.c.hit).sum(),
            miss: self.cards.iter().map(|c| c.c.miss).sum(),
            evict: self.cards.iter().map(|c| c.c.evict).sum(),
        };
        if sum.load != self.total.load
            || sum.unload != self.total.unload
            || sum.hit != self.total.hit
            || sum.miss != self.total.miss
            || sum.evict != self.total.evict
        {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    /// 全局命中率（千分比）。
    pub fn hit_rate_pm(&self) -> Result<u32, u16> {
        let acc = self.total.accesses();
        if acc == 0 {
            return Err(E_NO_ACCESS);
        }
        Ok((self.total.hit * 1000 / acc) as u32)
    }
    /// 热点 TopK：按访问量降序，同量按 id 升序。
    pub fn topk(&self, k: usize) -> Result<Vec<(u16, u64)>, u16> {
        if k == 0 || k > self.cards.len() {
            return Err(E_K);
        }
        let mut v: Vec<(u16, u64)> = self
            .cards
            .iter()
            .map(|c| (c.id, c.c.accesses()))
            .collect();
        // 选择排序取前 K（确定性）。
        for i in 0..k {
            let mut best = i;
            for j in i + 1..v.len() {
                let better = v[j].1 > v[best].1 || (v[j].1 == v[best].1 && v[j].0 < v[best].0);
                if better {
                    best = j;
                }
            }
            v.swap(i, best);
        }
        v.truncate(k);
        Ok(v)
    }
    /// 驻留场景数。
    pub fn resident_count(&self) -> usize {
        self.cards.iter().filter(|c| c.resident).count()
    }
    /// 卡只读视图。
    pub fn card(&self, id: u16) -> Option<&SceneCard> {
        self.cards.iter().find(|c| c.id == id)
    }
    /// 登记数。
    pub fn registered(&self) -> usize {
        self.cards.len()
    }
}

/// 热点报告行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotRow {
    /// 场景号。
    pub id: u16,
    /// 访问量。
    pub accesses: u64,
    /// 命中率（千分比）。
    pub hit_pm: u32,
}

/// TopK 报告化（含逐场景命中率；未命中样本算 0‰）。
pub fn hot_report(t: &SceneTelemetry, k: usize) -> Result<Vec<HotRow>, u16> {
    let top = t.topk(k)?;
    let mut out = Vec::with_capacity(top.len());
    for (id, acc) in top {
        let c = t.card(id).ok_or(E_UNKNOWN)?;
        let pm = if acc == 0 {
            0
        } else {
            (c.c.hit * 1000 / acc) as u32
        };
        out.push(HotRow {
            id,
            accesses: acc,
            hit_pm: pm,
        });
    }
    Ok(out)
}

/// 自例程：主路径（3 场景→加载/命中/未命中→审计→TopK）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut t = SceneTelemetry::new();
    t.register(10)?;
    t.register(20)?;
    t.register(30)?;
    if t.registered() != 3 {
        return Err(E_FULL);
    }
    // 错误路径 0：重号登记。
    if t.register(10) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 错误路径 1：未登记引用。
    if t.observe(99, SceneEvt::Load) != Err(E_UNKNOWN) {
        return Err(E_UNKNOWN);
    }
    // 主路径：10 加载 + 5 命中 + 3 未命中；20 加载 + 2 命中 + 8 未命中；
    // 30 仅登记未访问。
    t.observe(10, SceneEvt::Load)?;
    t.observe(20, SceneEvt::Load)?;
    for _ in 0..5 {
        t.observe(10, SceneEvt::Hit)?;
    }
    for _ in 0..3 {
        t.observe(10, SceneEvt::Miss)?;
    }
    for _ in 0..2 {
        t.observe(20, SceneEvt::Hit)?;
    }
    for _ in 0..8 {
        t.observe(20, SceneEvt::Miss)?;
    }
    t.audit()?;
    if t.resident_count() != 2 {
        return Err(E_LEDGER);
    }
    // 全局命中率：hit=7, miss=11 → 7000/18 = 388。
    if t.hit_rate_pm()? != 388 {
        return Err(E_NO_ACCESS);
    }
    // TopK：10 访问 8，20 访问 10 → 20 第一。
    let top = t.topk(2)?;
    if top[0].0 != 20 || top[0].1 != 10 || top[1].0 != 10 || top[1].1 != 8 {
        return Err(E_K);
    }
    // TopK 越界。
    if t.topk(0) != Err(E_K) || t.topk(4) != Err(E_K) {
        return Err(E_K);
    }
    // 热点报告：20 命中率 2*1000/10=200。
    let hr = hot_report(&t, 1)?;
    if hr[0].id != 20 || hr[0].hit_pm != 200 {
        return Err(E_K);
    }
    // 错误路径 2：重复加载。
    if t.observe(10, SceneEvt::Load) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 错误路径 3：卸载未驻留。
    if t.observe(30, SceneEvt::Unload) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 错误路径 4：未驻留命中。
    if t.observe(30, SceneEvt::Hit) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 主路径：逐出路径 + 守恒。
    t.observe(10, SceneEvt::Evict)?;
    t.audit()?;
    let c10 = t.card(10).ok_or(E_UNKNOWN)?;
    // load1 - unload0 - evict1 == 0 == resident(false)。
    if c10.evicted != 1 || c10.resident {
        return Err(E_LEDGER);
    }
    if t.resident_count() != 1 {
        return Err(E_LEDGER);
    }
    // 错误路径 5：空账命中率。
    let t0 = SceneTelemetry::new();
    if t0.hit_rate_pm() != Err(E_NO_ACCESS) {
        return Err(E_NO_ACCESS);
    }
    Ok(0)
}
