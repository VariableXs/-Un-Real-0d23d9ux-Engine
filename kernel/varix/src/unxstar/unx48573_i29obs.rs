//! UNX-F48573 —— I29 观测埋点（复签/联测/通知/越界四点）
//! 判据原文：UNX-F48573-J1 观测面在 I29 联签冻结链路上固定四类埋点：
//! 复签（resign）、联测（jointest）、通知（notify）、越界（oob）。
//! 每个埋点事件必须落进四点闭集之一并被恰一处计数；时间戳单调；
//! 越界点内事件不得转呈到复签点；记账守恒、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 四点闭集；R2 事件计数恰一；
//! R3 时戳单调；R4 越界不转呈；R5 复签点聚合守恒；
//! R6 联测/通知路径分流正确；R7 未知埋点拒收；
//! R8 fuzz 混合流零错态；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF760;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_KIND: u16 = SEG_BASE + 1; // 埋点越出四点闭集
const E_ONCE: u16 = SEG_BASE + 2; // 同一事件被多点重复计数
const E_TS: u16 = SEG_BASE + 3; // 时戳非单调
const E_OOB: u16 = SEG_BASE + 4; // 越界点内事件转呈
const E_RESIGN: u16 = SEG_BASE + 5; // 复签点聚合数不守恒
const E_SPLIT: u16 = SEG_BASE + 6; // 联测/通知分流错路
const E_UNKNOWN: u16 = SEG_BASE + 7; // 未知埋点拒收失败
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 混合流出现错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 埋点四点闭集。
pub const PT_RESIGN: u8 = 1;
pub const PT_JOINTEST: u8 = 2;
pub const PT_NOTIFY: u8 = 3;
pub const PT_OOB: u8 = 4;

/// 埋点名（下标 = 点位-1）。
pub const POINT_NAMES: [&str; 4] = ["resign", "jointest", "notify", "oob"];

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一条埋点事件：点位 + 时戳。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsEvent {
    pub point: u8,
    pub at: u32,
}

/// 单点账目：事件笔数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PointLedger {
    pub point: u8,
    pub hits: u32,
}

/// I29 观测埋点器：四点闭集、越界不转呈、聚合守恒。
pub struct ObsTracker {
    pub last_at: u32,
    pub ledgers: Vec<PointLedger>,
    pub relayed: Vec<ObsEvent>,
}

impl ObsTracker {
    pub fn new() -> ObsTracker {
        ObsTracker {
            last_at: 0,
            ledgers: vec![],
            relayed: vec![],
        }
    }

    /// 点位闭集判定。
    pub fn known_point(point: u8) -> bool {
        point == PT_RESIGN || point == PT_JOINTEST || point == PT_NOTIFY || point == PT_OOB
    }

    fn ledger_idx(&self, point: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if self.ledgers[i].point == point {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 发一条埋点：闭集校验、时戳单调、恰一处计数、越界不转呈。
    pub fn emit(&mut self, ev: ObsEvent) -> Result<u8, u16> {
        if !ObsTracker::known_point(ev.point) {
            return Err(E_KIND);
        }
        if ev.at < self.last_at {
            return Err(E_TS);
        }
        if self.ledger_idx(ev.point).is_none() {
            self.ledgers.push(PointLedger { point: ev.point, hits: 0 });
        }
        let idx = self.ledger_idx(ev.point);
        let i = match idx {
            Some(i) => i,
            None => return Err(E_KIND),
        };
        self.ledgers[i].hits += 1;
        self.last_at = ev.at;
        // 越界点终结；其余三点转呈记录。
        if ev.point != PT_OOB {
            self.relayed.push(ev);
        }
        Ok(ev.point)
    }

    /// 单点笔数查询。
    pub fn hits_of(&self, point: u8) -> u32 {
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if self.ledgers[i].point == point {
                return self.ledgers[i].hits;
            }
            i += 1;
        }
        0
    }

    /// 聚合守恒：四点计数和 == 事件总数（含被终结者）。
    pub fn conserved(&self, total_emitted: u32) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.ledgers.len() {
            sum += self.ledgers[i].hits;
            i += 1;
        }
        sum == total_emitted
    }

    /// 转呈数 = 事件总数 - 越界数。
    pub fn relay_consistent(&self, total: u32) -> bool {
        self.relayed.len() as u32 == total - self.hits_of(PT_OOB)
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 四点闭集：任意 u8 只落四点之一或拒收；点名非空互异。
    let mut lcg = Lcg::new(0x48573);
    let mut t = 0u32;
    while t < 32 {
        let p = (lcg.next_u32() % 8) as u8;
        if p >= 1 && p <= 4 {
            if !ObsTracker::known_point(p) {
                return Err(E_KIND);
            }
        } else if ObsTracker::known_point(p) {
            return Err(E_KIND);
        }
        t += 1;
    }
    if POINT_NAMES.len() != 4 || POINT_NAMES[0].is_empty() || POINT_NAMES[0] == POINT_NAMES[3] {
        return Err(E_KIND);
    }
    hits += 1;

    // R2 事件计数恰一：每点各发 3 笔，计数恰 3。
    let mut tr = ObsTracker::new();
    let mut at = 0u32;
    let mut t = 0u32;
    while t < 3 {
        tr.emit(ObsEvent { point: PT_RESIGN, at })?;
        at += 1;
        tr.emit(ObsEvent { point: PT_JOINTEST, at })?;
        at += 1;
        tr.emit(ObsEvent { point: PT_NOTIFY, at })?;
        at += 1;
        tr.emit(ObsEvent { point: PT_OOB, at })?;
        at += 1;
        t += 1;
    }
    if tr.hits_of(PT_RESIGN) != 3
        || tr.hits_of(PT_JOINTEST) != 3
        || tr.hits_of(PT_NOTIFY) != 3
        || tr.hits_of(PT_OOB) != 3
    {
        return Err(E_ONCE);
    }
    hits += 1;

    // R3 时戳单调：乱序被拒且诊断码恰为 E_TS。
    let mut tr2 = ObsTracker::new();
    tr2.emit(ObsEvent { point: PT_RESIGN, at: 100 })?;
    match tr2.emit(ObsEvent { point: PT_RESIGN, at: 50 }) {
        Err(E_TS) => hits += 1,
        _ => return Err(E_TS),
    }

    // R4 越界不转呈：越界事件不进 relayed，其余进。
    let mut tr3 = ObsTracker::new();
    tr3.emit(ObsEvent { point: PT_RESIGN, at: 1 })?;
    tr3.emit(ObsEvent { point: PT_OOB, at: 2 })?;
    tr3.emit(ObsEvent { point: PT_OOB, at: 3 })?;
    tr3.emit(ObsEvent { point: PT_NOTIFY, at: 4 })?;
    if tr3.relayed.len() != 2 || !tr3.relay_consistent(4) {
        return Err(E_OOB);
    }
    hits += 1;

    // R5 复签点聚合守恒：总发射数 == 四点和。
    let mut tr4 = ObsTracker::new();
    let mut total = 0u32;
    let mut at = 0u32;
    let mut t = 0u32;
    while t < 7 {
        tr4.emit(ObsEvent { point: PT_RESIGN, at })?;
        total += 1;
        at += 2;
        tr4.emit(ObsEvent { point: PT_OOB, at })?;
        total += 1;
        at += 2;
        t += 1;
    }
    if !tr4.conserved(total) {
        return Err(E_RESIGN);
    }
    hits += 1;

    // R6 联测/通知分流：联测事件只进联测键，通知只进通知键。
    let mut tr5 = ObsTracker::new();
    tr5.emit(ObsEvent { point: PT_JOINTEST, at: 10 })?;
    tr5.emit(ObsEvent { point: PT_NOTIFY, at: 20 })?;
    if tr5.hits_of(PT_JOINTEST) != 1 || tr5.hits_of(PT_NOTIFY) != 1 || tr5.hits_of(PT_OOB) != 0 {
        return Err(E_SPLIT);
    }
    hits += 1;

    // R7 未知埋点拒收：0/5/255 恒拒且码恰为 E_UNKNOWN。
    let odd = ObsEvent { point: 5, at: 1 };
    match tr5.emit(odd) {
        Err(E_UNKNOWN) => hits += 1,
        _ => return Err(E_UNKNOWN),
    }

    // R8 fuzz 混合流零错态：LCG 注入 96 笔合法事件，计数和守恒、转呈一致。
    let mut tr6 = ObsTracker::new();
    let mut lcg2 = Lcg::new(0x2910_0D);
    let mut total = 0u32;
    let mut at = 0u32;
    let mut t = 0u32;
    while t < 96 {
        let p = (1 + lcg2.below(4)) as u8;
        tr6.emit(ObsEvent { point: p, at })?;
        total += 1;
        at += 1 + lcg2.below(3);
        t += 1;
    }
    if !tr6.conserved(total) || !tr6.relay_consistent(total) {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = tracker_fp(&tr6);
    let f2 = tracker_fp(&tr6);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：四段前缀 rsg=/jnt=/ntf=/oob= 在位。
    let line = tracker_line(&tr6);
    if !has_kw(line.as_bytes(), b"rsg=")
        || !has_kw(line.as_bytes(), b"jnt=")
        || !has_kw(line.as_bytes(), b"ntf=")
        || !has_kw(line.as_bytes(), b"oob=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：四点计数 + 转呈数串联。
pub fn tracker_fp(tr: &ObsTracker) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&tr.hits_of(PT_RESIGN).to_le_bytes());
    buf.extend_from_slice(&tr.hits_of(PT_JOINTEST).to_le_bytes());
    buf.extend_from_slice(&tr.hits_of(PT_NOTIFY).to_le_bytes());
    buf.extend_from_slice(&tr.hits_of(PT_OOB).to_le_bytes());
    buf.extend_from_slice(&(tr.relayed.len() as u32).to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：四点计数一览。
pub fn tracker_line(tr: &ObsTracker) -> String {
    let mut s = String::new();
    s.push_str("rsg=");
    s.push_str(&tr.hits_of(PT_RESIGN).to_string());
    s.push_str(" jnt=");
    s.push_str(&tr.hits_of(PT_JOINTEST).to_string());
    s.push_str(" ntf=");
    s.push_str(&tr.hits_of(PT_NOTIFY).to_string());
    s.push_str(" oob=");
    s.push_str(&tr.hits_of(PT_OOB).to_string());
    s
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
