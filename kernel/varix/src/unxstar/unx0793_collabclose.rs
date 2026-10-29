#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0793 · 跨域协作终账（协作清单全闭环核）
//! 域/批：CoRun·UNX A1 收官域（签收/协作/封域面 · W7 清单第 20 单）
//! 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥570 行；② 判据原文：UNX-F0793-J1 终账过（协作清单逐项：事项/
//!   协作域/状态三列全闭环 100%）且零挂起协作断言过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//! 设计纪律：① 协作事项三列建模：事项号（唯一）/ 协作域（1..=5 闭集）/
//!   状态（Open→InFlight→Closed 合法边闭集，跳段/回头边一律拒记）。
//! ② 全闭环断言：逐项状态 == Closed（100%）；零挂起断言：Open + InFlight
//!   计数全零（两计数守恒：Closed + Open + InFlight == 总数）。
//! ③ 事项闭环必须带结项哈希（非 0，禁虚闭环）；状态迁移留痕事件账只增不删。
//! ④ 状态机即事项状态机 + 终账机相位（Intake→Flowing→Audited→Settled）。
//! ⑤ 常量写死注来源；诊断码定位到事项号；禁浮点禁 std。
//! 诊断段声明：0x6160–0x617F 本单独占（清单 <SEG 0x6160>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

// 1. 诊断码段（本单独占 0x6160–0x617F）
pub const SEG_BASE: u16 = 0x6160; // 诊断码段基数，来源：W7 清单 UNX-F0793 行 <SEG 0x6160>
pub const SEG_TOP: u16 = 0x617F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 终账相位非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空事项批
pub const XC_DUP_TID: u16 = SEG_BASE + 3; // 事项号重复
pub const XC_DOMAIN_BAD: u16 = SEG_BASE + 4; // 协作域越出 1..=5
pub const XC_BAD_EDGE: u16 = SEG_BASE + 5; // 状态迁移非法边（跳段/回头）
pub const XC_ALREADY_CLOSED: u16 = SEG_BASE + 6; // 已闭项再迁移
pub const XC_CLOSE_NO_HASH: u16 = SEG_BASE + 7; // 闭项无结项哈希（虚闭环）
pub const XC_ITEM_SHORT: u16 = SEG_BASE + 8; // 事项数不足下限
pub const XC_OPEN_LEFT: u16 = SEG_BASE + 9; // 零挂起断言红（Open 残留）
pub const XC_INFLIGHT_LEFT: u16 = SEG_BASE + 10; // 零挂起断言红（InFlight 残留）
pub const XC_COUNT_MISMATCH: u16 = SEG_BASE + 11; // 三态计数守恒断裂
pub const XC_NOT_AUDITED: u16 = SEG_BASE + 12; // 未终账即落定
pub const XC_ALREADY_SETTLED: u16 = SEG_BASE + 13; // 已落定再落定
pub const XC_TOPIC_EMPTY: u16 = SEG_BASE + 14; // 事项题为空
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 15; // 摘要为空

// 2. 终账常量（写死，来源=判据原文/本单口径头注假设）
pub const ITEM_MIN: usize = 12; // 协作事项下限，头注假设
pub const ITEM_CAP: usize = 48; // 事项定容上限，头注假设
pub const DOMAIN_COUNT: u8 = 5; // 协作域闭集 1..=5，来源：五域消费面口径
pub const FLOW_CAP: usize = 6; // 单事项状态迁移留痕上限（防账膨胀），头注假设

/// 事项状态机（合法边闭集：Open→InFlight→Closed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemState {
    Open = 0,
    InFlight = 1,
    Closed = 2,
}
impl ItemState {
    pub fn can_step(self, next: ItemState) -> bool {  // 合法边：Open→InFlight、InFlight→Closed；其余（含自环/回头/跳段）非法。
        (self as u8) + 1 == next as u8
    }
    pub fn zh(self) -> &'static str {
        match self {
            ItemState::Open => "挂起",
            ItemState::InFlight => "进行中",
            ItemState::Closed => "已闭环",
        }
    }
}

/// 一条协作事项（三列：事项/协作域/状态）。
#[derive(Clone, Debug)]
pub struct CollabItem {
    pub tid: u32,        // 事项号（唯一）
    pub domain: u8,      // 协作域 1..=5
    pub topic: String,   // 事项题（非空）
    pub state: ItemState,
    pub close_hash: u64, // 结项哈希（Closed 时非 0）
}

/// 终账报告。
#[derive(Clone, Debug)]
pub struct AuditReport {
    pub total: u32,       // 事项总数
    pub closed: u32,      // 闭环数
    pub open: u32,        // 挂起数（断言：零）
    pub inflight: u32,    // 进行中数（断言：零）
    pub all_closed: bool, // 全闭环 100% 结论
    pub per_domain: [u32; 5], // 逐域闭环计数（桶和 == 闭环数）
}

/// 落定回执。
#[derive(Clone, Debug)]
pub struct SettleReceipt {
    pub items: u32,       // 事项数
    pub closed: u32,      // 闭环数
    pub transitions: u32, // 全程迁移留痕总数
    pub summary: String,  // 摘要（非空）
    pub fingerprint: u64, // 账面指纹
}

/// 终账机相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettleStage {
    Intake = 0,
    Flowing = 1,
    Audited = 2,
    Settled = 3,
}
impl SettleStage {
    pub fn can_step(self, next: SettleStage) -> bool {
        (self as u8) + 1 == next as u8
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub struct CollabLedger {  // 跨域协作终账机。
    stage: SettleStage,
    items: Vec<CollabItem>,
    transitions: u32, // 全程迁移留痕总数
    audit: Option<AuditReport>,
    settled: Option<SettleReceipt>,
    events: Vec<String>,
}
impl CollabLedger {
    pub fn new() -> Self {
        CollabLedger {
            stage: SettleStage::Intake,
            items: Vec::new(),
            transitions: 0,
            audit: None,
            settled: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> SettleStage {
        self.stage
    }
    pub fn item_count(&self) -> usize {
        self.items.len()
    }
    pub fn transition_count(&self) -> u32 {
        self.transitions
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn feed_items(&mut self, batch: &[(u32, u8, String)]) -> Result<u32, u16> {  // 事项喂入：号唯一、域闭集、题为非空、定容守卫。
        if self.stage != SettleStage::Intake {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for (tid, domain, topic) in batch.iter() {
            if self.items.len() >= ITEM_CAP {
                return Err(XC_ITEM_SHORT);
            }
            if *domain < 1 || *domain > DOMAIN_COUNT {
                return Err(XC_DOMAIN_BAD);
            }
            if topic.is_empty() {
                return Err(XC_TOPIC_EMPTY);
            }
            let mut i = 0usize;
            while i < self.items.len() {
                if self.items[i].tid == *tid {
                    return Err(XC_DUP_TID);
                }
                i += 1;
            }
            self.items.push(CollabItem {
                tid: *tid,
                domain: *domain,
                topic: topic.clone(),
                state: ItemState::Open,
                close_hash: 0,
            });
            added += 1;
        }
        if self.stage == SettleStage::Intake && !self.items.is_empty() {
            self.stage = SettleStage::Flowing;
        }
        self.events.push(alloc::format!("items +{}", added));
        Ok(added)
    }

    /// 事项号定位。
    fn find(&self, tid: u32) -> Result<usize, u16> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].tid == tid {
                return Ok(i);
            }
            i += 1;
        }
        Err(XC_DUP_TID)
    }
    pub fn advance(&mut self, tid: u32, next: ItemState, close_hash: u64) -> Result<u32, u16> {  // 状态迁移：合法边守卫 + 迁移留痕；闭项需结项哈希。
        if self.stage != SettleStage::Flowing {
            return Err(XC_STATE_INVALID);
        }
        let idx = self.find(tid)?;
        let cur = self.items[idx].state;
        if cur == ItemState::Closed {
            return Err(XC_ALREADY_CLOSED);
        }
        if !cur.can_step(next) {
            return Err(XC_BAD_EDGE);
        }
        if next == ItemState::Closed {
            if close_hash == 0 {
                return Err(XC_CLOSE_NO_HASH);
            }
            self.items[idx].close_hash = close_hash;
        }
        self.items[idx].state = next;
        self.transitions += 1;
        self.events
            .push(alloc::format!("item {} {}->{}", tid, cur as u8, next as u8));
        Ok(self.transitions)
    }
    pub fn item_of(&self, tid: u32) -> Option<&CollabItem> {  // 事项查询（只读）。
        match self.find(tid) {
            Ok(i) => Some(&self.items[i]),
            Err(_) => None,
        }
    }
    pub fn audit(&mut self) -> Result<AuditReport, u16> {  // 终账：三态计数守恒 + 逐域桶 + 全闭环/零挂起断言。
        if !self.stage.can_step(SettleStage::Audited) {
            return Err(XC_STATE_INVALID);
        }
        if self.items.len() < ITEM_MIN {
            return Err(XC_ITEM_SHORT);
        }
        let mut closed = 0u32;
        let mut open = 0u32;
        let mut inflight = 0u32;
        let mut per_domain = [0u32; 5];
        let mut i = 0usize;
        while i < self.items.len() {
            let it = &self.items[i];
            match it.state {
                ItemState::Closed => {
                    closed += 1;
                    if it.close_hash == 0 {
                        return Err(XC_CLOSE_NO_HASH);
                    }
                    if it.domain >= 1 && it.domain <= 5 {
                        per_domain[(it.domain - 1) as usize] += 1;
                    }
                }
                ItemState::Open => open += 1,
                ItemState::InFlight => inflight += 1,
            }
            i += 1;
        }
        // 三态守恒：closed + open + inflight == 总数。
        let total = closed + open + inflight;
        if total != self.items.len() as u32 {
            return Err(XC_COUNT_MISMATCH);
        }
        let mut dsum = 0u32;
        let mut d = 0usize;
        while d < 5 {
            dsum += per_domain[d];
            d += 1;
        }
        if dsum != closed {
            return Err(XC_COUNT_MISMATCH);
        }
        let rep = AuditReport {
            total,
            closed,
            open,
            inflight,
            all_closed: open == 0 && inflight == 0,
            per_domain,
        };
        self.audit = Some(rep.clone());
        self.stage = SettleStage::Audited;
        self.events
            .push(alloc::format!("audit closed={} open={} inflight={}", closed, open, inflight));
        Ok(rep)
    }
    pub fn last_audit(&self) -> Option<&AuditReport> {
        self.audit.as_ref()
    }
    pub fn settle(&mut self) -> Result<SettleReceipt, u16> {  // 落定：全闭环 + 零挂起 → 摘要定格（重落定红）。
        if !self.stage.can_step(SettleStage::Settled) {
            return Err(if self.settled.is_some() {
                XC_ALREADY_SETTLED
            } else {
                XC_NOT_AUDITED
            });
        }
        let rep = match &self.audit {
            Some(r) => r.clone(),
            None => return Err(XC_NOT_AUDITED),
        };
        if rep.open != 0 {
            return Err(XC_OPEN_LEFT);
        }
        if rep.inflight != 0 {
            return Err(XC_INFLIGHT_LEFT);
        }
        if !rep.all_closed {
            return Err(XC_OPEN_LEFT);
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0793-J1 终账过：协作清单 {} 项三列全闭环 100%（逐域闭环 {}），零挂起协作断言过，迁移留痕 {} 次",
            rep.total,
            {
                let mut s = 0u32;
                let mut d = 0usize;
                while d < 5 {
                    s += rep.per_domain[d];
                    d += 1;
                }
                s
            },
            self.transitions
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let rc = SettleReceipt {
            items: rep.total,
            closed: rep.closed,
            transitions: self.transitions,
            summary,
            fingerprint: fp,
        };
        self.settled = Some(rc.clone());
        self.stage = SettleStage::Settled;
        Ok(rc)
    }
    pub fn settled_of(&self) -> Option<&SettleReceipt> {
        self.settled.as_ref()
    }
    pub fn fingerprint(&self) -> u64 {  // 账面指纹。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = &self.items[i];
            h = fnv1a(h, it.tid as u64);
            h = fnv1a(h, it.domain as u64);
            h = fnv1a(h, it.state as u64);
            h = fnv1a(h, it.close_hash);
            i += 1;
        }
        h
    }
    pub fn domain_closed_view(&self, domain: u8) -> Result<Vec<u32>, u16> {  // 逐域闭环视图：domain → 闭环事项号清单（越域 Err）。
        if domain < 1 || domain > DOMAIN_COUNT {
            return Err(XC_DOMAIN_BAD);
        }
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].domain == domain && self.items[i].state == ItemState::Closed {
                out.push(self.items[i].tid);
            }
            i += 1;
        }
        Ok(out)
    }

    pub fn ledger_view(&self) -> Result<Vec<String>, u16> {  // 终账视图：逐行三列（写回面用）。
        if self.items.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            let it = &self.items[i];
            out.push(alloc::format!(
                "T{} 域{} [{}] {}",
                it.tid, it.domain, it.state.zh(), it.topic
            ));
            i += 1;
        }
        Ok(out)
    }

    pub fn state_histogram(&self) -> Result<[u32; 3], u16> {  // 三态直方图：[Open, InFlight, Closed] 桶计数（桶和 == 总数守恒）。
        let mut hist = [0u32; 3];
        let mut i = 0usize;
        while i < self.items.len() {
            hist[self.items[i].state as usize] += 1;
            i += 1;
        }
        if hist[0] + hist[1] + hist[2] != self.items.len() as u32 {
            return Err(XC_COUNT_MISMATCH);
        }
        Ok(hist)
    }

    /// 事项迁移留痕数：tid 的实际迁移次数（Open 起点，每迁移 +1）。
    pub fn transitions_of(&self, tid: u32) -> Result<u32, u16> {  // 守恒：Closed 事项迁移数 == 2（Open→InFlight→Closed 两步）。
        let it = match self.item_of(tid) {
            Some(x) => x,
            None => return Err(XC_DUP_TID),
        };
        Ok(match it.state {
            ItemState::Open => 0,
            ItemState::InFlight => 1,
            ItemState::Closed => 2,
        })
    }

    /// 挂起清单：全部非 Closed 事项号（终账前自查面）。
    pub fn pending_list(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].state != ItemState::Closed {
                out.push(self.items[i].tid);
            }
            i += 1;
        }
        out
    }
}
pub fn gen_items(n: usize) -> Vec<(u32, u8, String)> {  // 样本发生器：n 个确定性事项（题非空，域轮转）。
    let mut out: Vec<(u32, u8, String)> = Vec::new();
    let mut i = 0usize;
    while i < n {
        out.push((
            (i + 1) as u32,
            (i % 5) as u8 + 1,
            alloc::format!("collab_{:03}", i + 1),
        ));
        i += 1;
    }
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let items = gen_items(14);
    // —— 主路径：喂 14 项 → 全部 Open→InFlight→Closed → 终账 → 落定 ——
    let mut m = CollabLedger::new();
    m.feed_items(&items)?;
    if m.item_count() != 14 {
        return Err(XC_ITEM_SHORT);
    }
    let mut i = 0usize;
    while i < 14 {
        let tid = (i + 1) as u32;
        m.advance(tid, ItemState::InFlight, 0)?;
        m.advance(tid, ItemState::Closed, 0x6160_0000_0000_0000 | tid as u64)?;
        i += 1;
    }
    if m.transition_count() != 28 {
        return Err(XC_COUNT_MISMATCH);
    }
    let rep = m.audit()?;
    if rep.total != 14 || rep.closed != 14 || rep.open != 0 || rep.inflight != 0 || !rep.all_closed {
        return Err(XC_COUNT_MISMATCH);
    }
    let rc = m.settle()?;
    if rc.items != 14 || rc.closed != 14 || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if !matches!(m.settle(), Err(XC_ALREADY_SETTLED)) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：跳段迁移（Open→Closed）与回头边（Closed→InFlight）——
    let mut m2 = CollabLedger::new();
    m2.feed_items(&items)?;
    if !matches!(m2.advance(1, ItemState::Closed, 0x61), Err(XC_BAD_EDGE)) {
        return Err(XC_BAD_EDGE);
    }
    m2.advance(1, ItemState::InFlight, 0)?;
    m2.advance(1, ItemState::Closed, 0x61)?;
    if m2.advance(1, ItemState::InFlight, 0) != Err(XC_ALREADY_CLOSED) {
        return Err(XC_ALREADY_CLOSED);
    }
    // —— 错误路径 2：虚闭环（闭项哈希为零）——
    let mut m3 = CollabLedger::new();
    m3.feed_items(&items)?;
    m3.advance(2, ItemState::InFlight, 0)?;
    if m3.advance(2, ItemState::Closed, 0) != Err(XC_CLOSE_NO_HASH) {
        return Err(XC_CLOSE_NO_HASH);
    }
    // —— 错误路径 3：挂起残留 → 落定红 ——
    let mut m4 = CollabLedger::new();
    m4.feed_items(&items)?;
    let mut i = 0usize;
    while i < 13 {
        let tid = (i + 1) as u32;
        m4.advance(tid, ItemState::InFlight, 0)?;
        m4.advance(tid, ItemState::Closed, 0x6160_0000_0000_0000 | tid as u64)?;
        i += 1;
    }
    m4.audit()?;
    if !matches!(m4.settle(), Err(XC_OPEN_LEFT)) {
        return Err(XC_OPEN_LEFT);
    }
    // —— 错误路径 4：事项数不足 / 域越界 / 空题 / 重号 ——
    let mut m5 = CollabLedger::new();
    m5.feed_items(&gen_items(6))?;
    if !matches!(m5.audit(), Err(XC_ITEM_SHORT)) {
        return Err(XC_ITEM_SHORT);
    }
    let mut m6 = CollabLedger::new();
    let mut bad = gen_items(3);
    bad[0].1 = 9;
    if !matches!(m6.feed_items(&bad), Err(XC_DOMAIN_BAD)) {
        return Err(XC_DOMAIN_BAD);
    }
    let mut m7 = CollabLedger::new();
    let mut empty = vec![(1u32, 1u8, String::new())];
    if !matches!(m7.feed_items(&empty), Err(XC_TOPIC_EMPTY)) {
        return Err(XC_TOPIC_EMPTY);
    }
    let mut m8 = CollabLedger::new();
    let dup = vec![
        (1u32, 1u8, String::from("a")),
        (1u32, 2u8, String::from("b")),
    ];
    if !matches!(m8.feed_items(&dup), Err(XC_DUP_TID)) {
        return Err(XC_DUP_TID);
    }
    // —— 错误路径 5：未终账即落定 ——
    let mut m9 = CollabLedger::new();
    m9.feed_items(&items)?;
    let mut i = 0usize;
    while i < 14 {
        let tid = (i + 1) as u32;
        m9.advance(tid, ItemState::InFlight, 0)?;
        m9.advance(tid, ItemState::Closed, 0x6160_0000_0000_0000 | tid as u64)?;
        i += 1;
    }
    if !matches!(m9.settle(), Err(XC_NOT_AUDITED)) {
        return Err(XC_NOT_AUDITED);
    }
    // —— 查询 API：事项可解引 / 挂起清单 ——
    match m.item_of(7) {
        Some(it) if it.state == ItemState::Closed && it.close_hash != 0 => {}
        _ => return Err(XC_CLOSE_NO_HASH),
    }
    if m.item_of(999).is_some() {
        return Err(XC_DUP_TID);
    }
    if !m.pending_list().is_empty() {
        return Err(XC_OPEN_LEFT);
    }
    if m4.pending_list().len() != 1 {
        return Err(XC_OPEN_LEFT);
    }
    // —— 扩展 API 面：逐域闭环视图 / 账视图 ——
    let dv = m.domain_closed_view(1)?;
    if dv.len() != 3 {
        return Err(XC_COUNT_MISMATCH);
    }
    if !matches!(m.domain_closed_view(9), Err(XC_DOMAIN_BAD)) {
        return Err(XC_DOMAIN_BAD);
    }
    let lv = m.ledger_view()?;
    if lv.len() != 14 {
        return Err(XC_EMPTY_FEED);
    }
    // —— 三态直方图 / 迁移数：主账全闭 14 项、m4 单挂起 ——
    let hist = m.state_histogram()?;
    if hist[0] != 0 || hist[1] != 0 || hist[2] != 14 {
        return Err(XC_COUNT_MISMATCH);
    }
    let hist4 = m4.state_histogram()?;
    if hist4[0] != 1 || hist4[2] != 13 {
        return Err(XC_COUNT_MISMATCH);
    }
    if m.transitions_of(3)? != 2 || !matches!(m.transitions_of(999), Err(XC_DUP_TID)) {
        return Err(XC_DUP_TID);
    }
    Ok(0)
}
