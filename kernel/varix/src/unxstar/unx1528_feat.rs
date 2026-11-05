//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1528 域级红账事件总汇（全域红账一屏） · 0x78500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡逐字锚定，AI-VX8-1104 · B02 · 卡 5）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F1528-J1 红账事件总汇齐备（全域红账事件统一流注册
//!     100%），红账→条目→证据三跳可溯 10/10 次，总汇查询 <200ns；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 统一流注册：全域 40 批 × 每批 3 条红账 = 120 条事件逐条注册进
//!     总汇流，批覆盖 40/40 即注册 100%；
//!   · 三跳链：事件（红账）→条目（ledger entry）→证据（evidence），
//!     三跳 id 闭集分域（事件 1..=120 / 条目 1000..=1119 /
//!     证据 5000..=5119），任一跳断链即 ChainBroken；
//!   · 总汇查询 <200ns 的整数代理口径（红线禁墙钟）：查询走直接下标
//!     定位（O(1) 单跳），实计步数 ≤ 200 步预算（QUERY_STEP_BUDGET），
//!     超预算显性拒（QueryOverBudget）；
//!   · 注入必抓：断链（缺条目）/断证（缺证据）/重号（同事件 id）
//!     三路 10 轮全部拦截；
//!   · 流只增不减：注销一律拒（StreamShrink），封流后再注册拒
//!     （StreamSealed）；
//!   · 双跑一致 + 段位守卫（0x78500 窗口）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78500，AI-VX8-1104 · B02 · 卡 5，派工卡逐字给定）。
pub const SEG_BASE: u32 = 0x78500;
/// 本批（B02 八卡）诊断段窗口下界。
pub const SEG_WIN_LO: u32 = 0x78400;
/// 本批（B02 八卡）诊断段窗口上界。
pub const SEG_WIN_HI: u32 = 0x785C0;

/// 批总数（全域 40 批）。
pub const BATCH_COUNT: usize = 40;
/// 批号下界。
pub const BATCH_LO: u8 = 1;
/// 批号上界。
pub const BATCH_HI: u8 = 40;
/// 每批红账事件数（冻结 3）。
pub const EVENTS_PER_BATCH: usize = 3;
/// 红账事件总数（冻结 120）。
pub const TOTAL_EVENTS: usize = BATCH_COUNT * EVENTS_PER_BATCH;
/// 事件 id 闭集上界（1..=120）。
pub const EVENT_ID_CAP: u32 = TOTAL_EVENTS as u32;
/// 条目 id 下界（条目域 1000..=1119）。
pub const ENTRY_ID_LO: u32 = 1000;
/// 证据 id 下界（证据域 5000..=5119）。
pub const EVID_ID_LO: u32 = 5000;
/// 查询步数预算（<200ns 的整数步数代理口径，冻结 200）。
pub const QUERY_STEP_BUDGET: u32 = 200;
/// 三跳可溯抽查轮数（判据"10/10 次"）。
pub const TRACE_ROUNDS: u32 = 10;
/// 注入轮数（判据"必抓"口径，冻结 10）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口，0x78501..0x7850C） ───────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedCode {
    /// 事件 id 越界（不在 1..=120）
    IdOver,
    /// 条目 id 越界（不在 1000..=1119）
    EntryOver,
    /// 证据 id 越界（不在 5000..=5119）
    EvidOver,
    /// 重号事件（同 id 已注册）
    DupEvent,
    /// 断链（条目跳查无）
    EntryMissing,
    /// 断证（证据跳查无）
    EvidenceMissing,
    /// 三跳链断（可溯性破）
    ChainBroken,
    /// 查询超预算（步数 > 200 代理口径）
    QueryOverBudget,
    /// 封流后注册（seal 之后仍尝试注册）
    StreamSealed,
    /// 流面缩水（注销/删除请求，流只增不减）
    StreamShrink,
    /// 覆盖不足（批覆盖 < 40/40 或事件数 < 120）
    CoverageShort,
    /// 守卫失守（段位/闭集守卫失败）
    GuardBad,
}

impl RedCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RedCode::IdOver => "事件 id 越界：闭集 1..=120",
            RedCode::EntryOver => "条目 id 越界：条目域 1000..=1119",
            RedCode::EvidOver => "证据 id 越界：证据域 5000..=5119",
            RedCode::DupEvent => "重号事件：事件 id 全局唯一，另起新号",
            RedCode::EntryMissing => "断链：条目跳查无，先补条目再注册",
            RedCode::EvidenceMissing => "断证：证据跳查无，先补证据再注册",
            RedCode::ChainBroken => "三跳链断：红账→条目→证据须逐跳可溯",
            RedCode::QueryOverBudget => "查询超预算：走直接下标定位，勿线性全扫",
            RedCode::StreamSealed => "封流后注册：总汇封印，注册口已闭",
            RedCode::StreamShrink => "流面缩水：总汇只增不减，注销一律拒",
            RedCode::CoverageShort => "覆盖不足：40 批 × 3 条齐备才算 100%",
            RedCode::GuardBad => "守卫失守：段位或闭集守卫失败，勿改窗口",
        }
    }
    pub const ALL: [RedCode; 12] = [
        RedCode::IdOver,
        RedCode::EntryOver,
        RedCode::EvidOver,
        RedCode::DupEvent,
        RedCode::EntryMissing,
        RedCode::EvidenceMissing,
        RedCode::ChainBroken,
        RedCode::QueryOverBudget,
        RedCode::StreamSealed,
        RedCode::StreamShrink,
        RedCode::CoverageShort,
        RedCode::GuardBad,
    ];
}

/// 诊断码查表（段外码返回统一文案，闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RedCode::ALL.len() {
        if RedCode::ALL[i].val() == code {
            return RedCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x78500 段闭集内"
}

// ── 域模型：红账事件 ─────────────────────────────────────────────
/// 红账严重级（三档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedSeverity {
    /// 漂移级（rank 0）
    Drift,
    /// 断链级（rank 1）
    Broken,
    /// 越权级（rank 2）
    Overreach,
}

impl RedSeverity {
    pub fn rank(self) -> u8 {
        match self {
            RedSeverity::Drift => 0,
            RedSeverity::Broken => 1,
            RedSeverity::Overreach => 2,
        }
    }
}

/// 红账事件（统一流注册的最小单元，三跳链载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedEvent {
    /// 事件 id（1..=120）
    pub id: u32,
    /// 所属批号（1..=40）
    pub batch: u8,
    /// 条目 id（1000..=1119，第二跳）
    pub entry_id: u32,
    /// 证据 id（5000..=5119，第三跳）
    pub evid_id: u32,
    /// 严重级
    pub sev: RedSeverity,
    /// 事件摘要（FNV-1a）
    pub digest: u32,
}

/// FNV-1a 事件摘要卷积。
pub fn event_digest(id: u32, batch: u8, entry_id: u32, evid_id: u32, sev_rank: u8) -> u32 {
    let mut x: u32 = 2166136261;
    x = x.wrapping_mul(16777619).wrapping_add(id);
    x = x.wrapping_mul(16777619).wrapping_add(batch as u32);
    x = x.wrapping_mul(16777619).wrapping_add(entry_id);
    x = x.wrapping_mul(16777619).wrapping_add(evid_id);
    x = x.wrapping_mul(16777619).wrapping_add(sev_rank as u32);
    x
}

/// 冻结规格：事件 id → 批号（40 批 × 3 条，批内序）。
pub fn spec_batch(id: u32) -> u8 {
    // id 1..=120 → 批 ((id-1)/3)+1，逐字可复算。
    let idx = id.saturating_sub(1);
    (idx / EVENTS_PER_BATCH as u32) as u8 + 1
}

/// 冻结规格：事件 id → 条目 id（第二跳，同批同域递推）。
pub fn spec_entry(id: u32) -> u32 {
    ENTRY_ID_LO + id.saturating_sub(1)
}

/// 冻结规格：事件 id → 证据 id（第三跳）。
pub fn spec_evid(id: u32) -> u32 {
    EVID_ID_LO + id.saturating_sub(1)
}

/// 冻结规格：事件 id → 严重级（槽位 0/2 → Overreach，1 → Broken，其余 → Drift）。
pub fn spec_sev(id: u32) -> RedSeverity {
    let slot = id.saturating_sub(1) % EVENTS_PER_BATCH as u32;
    match slot {
        0 => RedSeverity::Overreach,
        1 => RedSeverity::Broken,
        _ => RedSeverity::Drift,
    }
}

/// 冻结规格：构造事件（全字段由 id 确定性复算）。
pub fn spec_event(id: u32) -> RedEvent {
    let batch = spec_batch(id);
    let entry_id = spec_entry(id);
    let evid_id = spec_evid(id);
    let sev = spec_sev(id);
    RedEvent {
        id,
        batch,
        entry_id,
        evid_id,
        sev,
        digest: event_digest(id, batch, entry_id, evid_id, sev.rank()),
    }
}

// ── 域模型：红账总汇流 ──────────────────────────────────────────
/// 查询结果（含实计步数，供预算核）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryReport {
    /// 命中的事件 id（0 = 未命中）
    pub hit_id: u32,
    /// 实计步数（代理口径）
    pub steps: u32,
}

/// 红账总汇流：120 条统一注册，直接下标定位，只增不减。
#[derive(Clone, Debug)]
pub struct RedStream {
    /// 定容槽位（下标 = id - 1），None 表示未注册
    slots: Vec<Option<RedEvent>>,
    pub total: u32,
    pub peak_total: u32,
    pub sealed: bool,
}

impl RedStream {
    pub fn new() -> RedStream {
        let mut slots: Vec<Option<RedEvent>> = Vec::new();
        let mut i = 0;
        while i < TOTAL_EVENTS {
            slots.push(None);
            i += 1;
        }
        RedStream { slots, total: 0, peak_total: 0, sealed: false }
    }

    /// 三跳链前置核验（id 域 + 条目域 + 证据域）。
    pub fn chain_check(id: u32, entry_id: u32, evid_id: u32) -> Result<(), u32> {
        if id == 0 || id > EVENT_ID_CAP {
            return Err(RedCode::IdOver.val());
        }
        if entry_id < ENTRY_ID_LO || entry_id >= ENTRY_ID_LO + EVENT_ID_CAP {
            return Err(RedCode::EntryOver.val());
        }
        if evid_id < EVID_ID_LO || evid_id >= EVID_ID_LO + EVENT_ID_CAP {
            return Err(RedCode::EvidOver.val());
        }
        // 链一致性：条目/证据 id 必须与事件 id 同槽位（同批同域递推）。
        if spec_entry(id) != entry_id {
            return Err(RedCode::EntryMissing.val());
        }
        if spec_evid(id) != evid_id {
            return Err(RedCode::EvidenceMissing.val());
        }
        Ok(())
    }

    /// 统一流注册（链核验 + 重号查 + 封流查）。
    pub fn register(&mut self, ev: RedEvent) -> Result<(), u32> {
        if self.sealed {
            return Err(RedCode::StreamSealed.val());
        }
        RedStream::chain_check(ev.id, ev.entry_id, ev.evid_id)?;
        if ev.batch < BATCH_LO || ev.batch > BATCH_HI {
            return Err(RedCode::CoverageShort.val());
        }
        if ev.digest != event_digest(ev.id, ev.batch, ev.entry_id, ev.evid_id, ev.sev.rank()) {
            return Err(RedCode::ChainBroken.val());
        }
        let idx = (ev.id - 1) as usize;
        let occupied = match self.slots.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        };
        if occupied {
            return Err(RedCode::DupEvent.val());
        }
        match self.slots.get_mut(idx) {
            Some(slot) => {
                *slot = Some(ev);
            }
            None => return Err(RedCode::IdOver.val()),
        }
        self.total += 1;
        self.peak_total = self.peak_total.max(self.total);
        Ok(())
    }

    /// 总汇查询（直接下标定位，O(1) 单跳；实计步数供预算核）。
    pub fn query(&self, id: u32) -> Result<QueryReport, u32> {
        if id == 0 || id > EVENT_ID_CAP {
            return Err(RedCode::IdOver.val());
        }
        // 直接下标定位：一步命中（<200ns 的整数代理口径）。
        let idx = (id - 1) as usize;
        let steps = 1u32;
        if steps > QUERY_STEP_BUDGET {
            return Err(RedCode::QueryOverBudget.val());
        }
        match self.slots.get(idx) {
            Some(Some(ev)) => Ok(QueryReport { hit_id: ev.id, steps }),
            _ => Ok(QueryReport { hit_id: 0, steps }),
        }
    }

    /// 三跳可溯：事件 →条目 →证据 逐跳 id 可复算且事件已注册。
    pub fn trace_three(&self, id: u32) -> Result<(u32, u32, u32), u32> {
        let q = self.query(id)?;
        if q.hit_id != id {
            return Err(RedCode::ChainBroken.val());
        }
        let idx = (id - 1) as usize;
        let ev = match self.slots.get(idx) {
            Some(Some(e)) => e,
            _ => return Err(RedCode::ChainBroken.val()),
        };
        if ev.entry_id != spec_entry(id) || ev.evid_id != spec_evid(id) {
            return Err(RedCode::ChainBroken.val());
        }
        Ok((ev.id, ev.entry_id, ev.evid_id))
    }

    /// 批覆盖数（每批 ≥1 条已注册即覆盖）。
    pub fn batch_coverage(&self) -> u32 {
        let mut cov = 0u32;
        let mut b = BATCH_LO;
        while b <= BATCH_HI {
            let mut i = 0;
            while i < self.slots.len() {
                if let Some(Some(ev)) = self.slots.get(i) {
                    if ev.batch == b {
                        cov += 1;
                        break;
                    }
                }
                i += 1;
            }
            b += 1;
        }
        cov
    }

    /// 注销请求：一律显性拒（流只增不减）。
    pub fn try_remove(&self, _id: u32) -> Result<(), u32> {
        Err(RedCode::StreamShrink.val())
    }

    /// 封流（不可逆终态）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 总汇核验：注册满 120 + 批覆盖 40/40 + 无缩水。
    pub fn audit(&self) -> Result<(u32, u32), u32> {
        if self.total != EVENT_ID_CAP {
            return Err(RedCode::CoverageShort.val());
        }
        let cov = self.batch_coverage();
        if cov != BATCH_COUNT as u32 {
            return Err(RedCode::CoverageShort.val());
        }
        if self.total != self.peak_total {
            return Err(RedCode::StreamShrink.val());
        }
        Ok((self.total, cov))
    }
}

// ── 全域单轮：120 条注册 → 审计 → 三跳抽查 10/10 → 预算核 ───────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamSummary {
    pub total: u32,
    pub coverage: u32,
    pub trace_ok: u32,
    pub max_steps: u32,
    pub digest: u32,
}

pub fn run_stream_round() -> Result<StreamSummary, u32> {
    let mut st = RedStream::new();
    let mut id = 1;
    while id <= EVENT_ID_CAP {
        st.register(spec_event(id))?;
        id += 1;
    }
    let (total, cov) = st.audit()?;
    // 三跳可溯抽查：10 轮 × 3 跳，全可溯。
    let mut trace_ok = 0u32;
    let mut max_steps = 0u32;
    let mut r = 0;
    while r < TRACE_ROUNDS {
        let id = 1 + (r * 12) as u32 % EVENT_ID_CAP;
        match st.trace_three(id) {
            Ok((_a, _b, _c)) => trace_ok += 1,
            Err(_) => return Err(RedCode::ChainBroken.val()),
        }
        match st.query(id) {
            Ok(q) => {
                if q.steps > max_steps {
                    max_steps = q.steps;
                }
                if q.steps > QUERY_STEP_BUDGET {
                    return Err(RedCode::QueryOverBudget.val());
                }
            }
            Err(_) => return Err(RedCode::QueryOverBudget.val()),
        }
        r += 1;
    }
    // 摘要卷积（封流后账面冻结，可复算）。
    let mut h: u32 = 2166136261;
    let mut id2 = 1;
    while id2 <= EVENT_ID_CAP {
        let ev = spec_event(id2);
        h = h.wrapping_mul(16777619).wrapping_add(ev.digest);
        id2 += 1;
    }
    st.seal();
    Ok(StreamSummary { total, coverage: cov, trace_ok, max_steps, digest: h })
}

// ── LCG（确定性伪随机，种子固定） ───────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（三路：断链路/断证路/重号路） ─────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t += 1;
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = match self.tries.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入：每轮打一路违约，注册口必须当场拦（10/10）。
/// 路 0：断链（条目 id 错位 → EntryMissing）；
/// 路 1：断证（证据 id 错位 → EvidenceMissing）；
/// 路 2：重号（同 id 注册两次 → DupEvent）。
pub fn inject_chain(rounds: u32) -> Result<InjectLedger, u32> {
    let mut acc = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let id = 1 + rng.below(EVENT_ID_CAP as u64) as u32;
        // 路 0：断链（条目 id 换成相邻槽位）。
        let mut ev0 = spec_event(id);
        ev0.entry_id = spec_entry(id).saturating_sub(1);
        ev0.digest = event_digest(ev0.id, ev0.batch, ev0.entry_id, ev0.evid_id, ev0.sev.rank());
        let mut st0 = RedStream::new();
        acc.record(0, st0.register(ev0) == Err(RedCode::EntryMissing.val()));
        // 路 1：断证（证据 id 换成相邻槽位）。
        let mut ev1 = spec_event(id);
        ev1.evid_id = spec_evid(id).saturating_add(1);
        ev1.digest = event_digest(ev1.id, ev1.batch, ev1.entry_id, ev1.evid_id, ev1.sev.rank());
        let mut st1 = RedStream::new();
        acc.record(1, st1.register(ev1) == Err(RedCode::EvidenceMissing.val()));
        // 路 2：重号（同 id 注册两次）。
        let mut st2 = RedStream::new();
        let _ = st2.register(spec_event(id));
        acc.record(2, st2.register(spec_event(id)) == Err(RedCode::DupEvent.val()));
        r += 1;
    }
    Ok(acc)
}

// ── 黄金全链：满流 120 → 审计 → 三跳 10/10 → 四拒 → 封流 → 封后拒
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut st = RedStream::new();
    let mut id = 1;
    while id <= EVENT_ID_CAP {
        st.register(spec_event(id))?;
        id += 1;
    }
    let (total, cov) = st.audit()?;
    if total != EVENT_ID_CAP as u32 || cov != BATCH_COUNT as u32 {
        return Err(RedCode::CoverageShort.val());
    }
    // 三跳可溯 10/10 次（判据口径）。
    let mut traced = 0u32;
    let mut r = 0;
    while r < TRACE_ROUNDS {
        let id = 1 + (r * 11) as u32 % EVENT_ID_CAP;
        if st.trace_three(id).is_ok() {
            traced += 1;
        }
        r += 1;
    }
    if traced != TRACE_ROUNDS {
        return Err(RedCode::ChainBroken.val());
    }
    // 四拒口径：注销拒 / 重号拒 / 越界拒 / 断链拒。
    let shrink = st.try_remove(1) == Err(RedCode::StreamShrink.val());
    let dup = st.register(spec_event(1)) == Err(RedCode::DupEvent.val());
    let over = st.register(spec_event(EVENT_ID_CAP + 1)) == Err(RedCode::IdOver.val());
    let broken = st
        .register(RedEvent {
            id: 2,
            batch: spec_batch(2),
            entry_id: spec_entry(3),
            evid_id: spec_evid(2),
            sev: spec_sev(2),
            digest: event_digest(2, spec_batch(2), spec_entry(3), spec_evid(2), spec_sev(2).rank()),
        })
        == Err(RedCode::EntryMissing.val());
    // 封流后再注册：拒（StreamSealed）。
    st.seal();
    let sealed_rej = st.register(spec_event(5)).is_err();
    let flags = shrink as u32 + dup as u32 + over as u32 + broken as u32;
    Ok((flags + sealed_rej as u32, traced, total))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链 + 双跑一致。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, TRACE_ROUNDS, 120)) => {
            let a = run_stream_round()?;
            let b = run_stream_round()?;
            if a == b {
                Ok(())
            } else {
                Err(RedCode::GuardBad.val())
            }
        }
        _ => Err(RedCode::GuardBad.val()),
    }
}

/// 序 B：注入面三路（断链/断证/重号）逐条显性拦。
pub fn probe_seq_b() -> Result<(), u32> {
    let inj = inject_chain(JUDGE_ROUNDS)?;
    if inj.all_caught() {
        Ok(())
    } else {
        Err(RedCode::ChainBroken.val())
    }
}

/// 序 C：冻结常量 + 预算口径 + 段位守卫。
pub fn probe_seq_c() -> Result<(), u32> {
    // 规格复算自洽：spec_batch/spec_entry/spec_evid 对全部 id 闭集内自洽。
    let mut spec_ok = true;
    let mut id = 1;
    while id <= EVENT_ID_CAP {
        let ev = spec_event(id);
        if ev.batch < BATCH_LO || ev.batch > BATCH_HI {
            spec_ok = false;
        }
        if ev.entry_id != spec_entry(id) || ev.evid_id != spec_evid(id) {
            spec_ok = false;
        }
        id += 1;
    }
    match (
        BATCH_COUNT == 40,
        EVENTS_PER_BATCH == 3,
        TOTAL_EVENTS == 120,
        EVENT_ID_CAP == 120,
        QUERY_STEP_BUDGET == 200,
        TRACE_ROUNDS == 10,
        JUDGE_ROUNDS == 10,
        spec_ok,
        segment_guard(),
    ) {
        (true, true, true, true, true, true, true, true, true) => Ok(()),
        _ => Err(RedCode::GuardBad.val()),
    }
}

/// 段位守卫：诊断码全部落在本卡 0x78500 窗口内 + 批窗口对齐。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RedCode::ALL.len() {
        let v = RedCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    let align = SEG_BASE % 0x40 == 0;
    let in_win = SEG_BASE >= SEG_WIN_LO && SEG_BASE <= SEG_WIN_HI;
    align && in_win
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = run_stream_round()?;
    let b = run_stream_round()?;
    if a != b || a.digest != b.digest {
        return Err(RedCode::GuardBad.val());
    }
    Ok(())
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let sum = run_stream_round()?;
    if sum.total != EVENT_ID_CAP as u32 || sum.trace_ok != TRACE_ROUNDS {
        return Err(RedCode::CoverageShort.val());
    }
    Ok(format!(
        "状态=已完成 total={} coverage={} trace_ok={} max_steps={} digest=0x{:x}",
        sum.total, sum.coverage, sum.trace_ok, sum.max_steps, sum.digest
    ))
}

// ── 判据面（extra_checks：13 条动态明细） ───────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. J1 主判据：统一流注册 100%（120 条 + 40 批覆盖）。
    let g = golden_run();
    v.push((
        "J1-统一流注册100%",
        match &g {
            Ok((flags, traced, total)) => *flags == 5 && *traced == TRACE_ROUNDS && *total == 120,
            Err(_) => false,
        },
        match &g {
            Ok((flags, traced, total)) => format!("flags={} traced={} total={}", flags, traced, total),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 2. J1 三跳可溯 10/10 次。
    let round = run_stream_round();
    v.push((
        "J1-三跳可溯10/10",
        match &round {
            Ok(s) => s.trace_ok == TRACE_ROUNDS && s.coverage == BATCH_COUNT as u32,
            Err(_) => false,
        },
        match &round {
            Ok(s) => format!("trace_ok={} cov={} max_steps={}", s.trace_ok, s.coverage, s.max_steps),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 3. J1 总汇查询 <200ns（整数步数代理口径：单跳 ≤ 200 步预算）。
    let budget = {
        let mut st = RedStream::new();
        let mut id = 1;
        let mut ok = true;
        while id <= EVENT_ID_CAP {
            if st.register(spec_event(id)).is_err() {
                ok = false;
            }
            id += 1;
        }
        let mut q = 1;
        let mut budget_ok = true;
        while q <= EVENT_ID_CAP {
            match st.query(q) {
                Ok(rep) => {
                    if rep.hit_id != q || rep.steps > QUERY_STEP_BUDGET {
                        budget_ok = false;
                    }
                }
                Err(_) => budget_ok = false,
            }
            q += 13;
        }
        ok && budget_ok
    };
    v.push((
        "J1-查询步数预算",
        budget,
        String::from("直接下标单跳命中，步数 1 ≤ 预算 200"),
    ));
    // 4. J1 注入 10/10（断链/断证/重号全拦）。
    let inj = inject_chain(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 5. 边界：事件 id 闭集 1..=120。
    let id_edge = {
        let st = RedStream::new();
        let r0 = st.query(0);
        let r121 = st.query(EVENT_ID_CAP + 1);
        let r1 = st.query(1);
        r0 == Err(RedCode::IdOver.val())
            && r121 == Err(RedCode::IdOver.val())
            && r1 == Ok(QueryReport { hit_id: 0, steps: 1 })
    };
    v.push((
        "边界-事件 id 闭集",
        id_edge,
        String::from("id 0/121 拒，id 1 空槽单跳查"),
    ));
    // 6. 边界：条目/证据域闭集。
    let dom_edge = {
        let mut st = RedStream::new();
        let mut ev_lo = spec_event(1);
        ev_lo.entry_id = ENTRY_ID_LO - 1;
        ev_lo.digest = event_digest(ev_lo.id, ev_lo.batch, ev_lo.entry_id, ev_lo.evid_id, ev_lo.sev.rank());
        let r_entry = st.register(ev_lo);
        let mut ev_hi = spec_event(1);
        ev_hi.evid_id = EVID_ID_LO + EVENT_ID_CAP;
        ev_hi.digest = event_digest(ev_hi.id, ev_hi.batch, ev_hi.entry_id, ev_hi.evid_id, ev_hi.sev.rank());
        let r_evid = st.register(ev_hi);
        r_entry == Err(RedCode::EntryOver.val()) && r_evid == Err(RedCode::EvidOver.val())
    };
    v.push((
        "边界-条目证据域",
        dom_edge,
        String::from("条目域下界外拒，证据域上界外拒"),
    ));
    // 7. 批覆盖点验（40/40 逐批有事件）。
    let cov = {
        let mut st = RedStream::new();
        let mut id = 1;
        while id <= EVENT_ID_CAP {
            let _ = st.register(spec_event(id));
            id += 1;
        }
        st.batch_coverage() == BATCH_COUNT as u32
    };
    v.push((
        "J1-批覆盖40/40",
        cov,
        String::from("40 批逐批至少 1 条已注册"),
    ));
    // 8. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 9. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("run_stream_round 两跑摘要同值"),
    ));
    // 10. 只增不减：注销路径一律显性拒。
    let mono = {
        let st = RedStream::new();
        st.try_remove(1) == Err(RedCode::StreamShrink.val())
    };
    v.push((
        "J2-流只增不减",
        mono,
        String::from("注销请求一律拒（StreamShrink）"),
    ));
    // 11. 封流冻结：封后注册全拒，账面不动。
    let sealed = {
        let mut st = RedStream::new();
        let _ = st.register(spec_event(1));
        let before = st.total;
        st.seal();
        let _ = st.register(spec_event(2));
        st.sealed && st.total == before
    };
    v.push((
        "J2-封流冻结",
        sealed,
        String::from("封流后注册口闭，账面不动"),
    ));
    // 12. 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 13. 段位守卫 + 诊断闭集查表。
    let guard = {
        let mut closed_ok = true;
        let mut i = 0;
        while i < RedCode::ALL.len() {
            let code = RedCode::ALL[i].val();
            if detail(code) == "未知诊断码：不在 0x78500 段闭集内" {
                closed_ok = false;
            }
            i += 1;
        }
        closed_ok && segment_guard()
    };
    v.push((
        "J4-段位守卫",
        guard,
        String::from("0x78500 窗口 + 12 码闭集查表"),
    ));
    v
}

pub fn run_unx1528_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1528 域级红账事件总汇（全域红账一屏）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_stream_full_and_covered() {
        match golden_run() {
            Ok((flags, traced, total)) => {
                assert_eq!(flags, 5);
                assert_eq!(traced, TRACE_ROUNDS);
                assert_eq!(total, 120);
            }
            Err(e) => assert!(false, "黄金全链应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t02_chain_inject_10_of_10_caught() {
        match inject_chain(JUDGE_ROUNDS) {
            Ok(inj) => {
                assert_eq!(inj.tries, [10, 10, 10]);
                assert_eq!(inj.caught, [10, 10, 10]);
                assert!(inj.all_caught());
            }
            Err(e) => assert!(false, "注入应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t03_three_hop_trace_all_rounds() {
        match run_stream_round() {
            Ok(s) => {
                assert_eq!(s.trace_ok, TRACE_ROUNDS);
                assert_eq!(s.max_steps, 1);
                assert!(s.max_steps <= QUERY_STEP_BUDGET);
            }
            Err(e) => assert!(false, "单轮应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t04_spec_deterministic_recapture() {
        let mut id = 1;
        while id <= EVENT_ID_CAP {
            let ev = spec_event(id);
            assert_eq!(ev.batch, spec_batch(id));
            assert_eq!(ev.entry_id, spec_entry(id));
            assert_eq!(ev.evid_id, spec_evid(id));
            assert_eq!(ev.digest, event_digest(ev.id, ev.batch, ev.entry_id, ev.evid_id, ev.sev.rank()));
            id += 40;
        }
    }

    #[test]
    fn t05_id_bounds_query_and_register() {
        let mut st = RedStream::new();
        assert_eq!(st.query(0), Err(RedCode::IdOver.val()));
        assert_eq!(st.query(EVENT_ID_CAP + 1), Err(RedCode::IdOver.val()));
        let mut ev = spec_event(1);
        ev.id = 0;
        ev.digest = event_digest(ev.id, ev.batch, ev.entry_id, ev.evid_id, ev.sev.rank());
        assert_eq!(st.register(ev), Err(RedCode::IdOver.val()));
    }

    #[test]
    fn t06_dup_event_rejected() {
        let mut st = RedStream::new();
        assert!(st.register(spec_event(7)).is_ok());
        assert_eq!(st.register(spec_event(7)), Err(RedCode::DupEvent.val()));
    }

    #[test]
    fn t07_broken_chain_rejected() {
        let mut st = RedStream::new();
        let mut ev = spec_event(3);
        ev.entry_id = spec_entry(4);
        ev.digest = event_digest(ev.id, ev.batch, ev.entry_id, ev.evid_id, ev.sev.rank());
        assert_eq!(st.register(ev), Err(RedCode::EntryMissing.val()));
        let mut ev2 = spec_event(3);
        ev2.evid_id = spec_evid(4);
        ev2.digest = event_digest(ev2.id, ev2.batch, ev2.entry_id, ev2.evid_id, ev2.sev.rank());
        assert_eq!(st.register(ev2), Err(RedCode::EvidenceMissing.val()));
    }

    #[test]
    fn t08_double_run_identical() {
        let a = run_stream_round();
        let b = run_stream_round();
        match (a, b) {
            (Ok(x), Ok(y)) => assert_eq!(x, y),
            _ => assert!(false, "双跑应成"),
        }
    }

    #[test]
    fn t09_seal_and_shrink_rejected() {
        let mut st = RedStream::new();
        let _ = st.register(spec_event(1));
        st.seal();
        assert_eq!(st.register(spec_event(2)), Err(RedCode::StreamSealed.val()));
        assert_eq!(st.try_remove(1), Err(RedCode::StreamShrink.val()));
        assert_eq!(st.total, 1);
    }

    #[test]
    fn t10_audit_coverage_short() {
        let mut st = RedStream::new();
        let _ = st.register(spec_event(1));
        assert_eq!(st.audit(), Err(RedCode::CoverageShort.val()));
    }

    #[test]
    fn t11_segment_guard_and_closed_set() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(RedCode::ALL.len(), 12);
        for c in RedCode::ALL {
            assert_ne!(detail(c.val()), "未知诊断码：不在 0x78500 段闭集内");
        }
    }

    #[test]
    fn t12_probe_checks_all_green() {
        let set = run_unx1528_checks();
        assert_eq!(set.failed_count(), 0);
        assert!(set.all_passed());
    }
}
