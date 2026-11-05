//! # unx49081 — 恶意设备防御总成（X3 三段限速收口 + 洪泛取证面）与热移除错误路径全路径
//!
//! 卡面原文（任务板 B15·F49081 / F49082）：
//!
//! > **F49081 恶意设备防御总成（X3 限速执行器收口+洪泛取证面）**
//! > ① 纯功能代码 ≥400 行；② 判据原文：UNX-F49082-J1 热移除 ×10 终验零残留；
//! > 判据锚定 §AI-62 §2 专题一错误路径；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! > **F49082 热移除错误路径全路径（设备节点热移除在途帧丢弃入账终验）**
//! > ① 纯功能代码 ≥380 行；② 判据原文：UNX-F49083-J1 乱序 ×10 重置流程全过；
//! > 判据锚定 §AI-62 §2 专题一错误路径。
//!
//! ## 判据锚点
//!
//! - `UNX-F49082-J1` 热移除 ×10 终验零残留：队列 0、引用 0、丢弃 == 入队 − 交付，三者同时成立。
//! - `UNX-F49083-J1` 乱序 ×10 重置流程全过：交付 + 冲账 == 计划，残留 0，重置恰好一次。
//! - 两条判据均锚定 §AI-62 §2 专题一「错误路径」：问的不是稳态吞吐，而是拔线瞬间账还平不平。
//!
//! ## 闸序与理由
//!
//! 1. **段位与码面**：14 码落 `0x73501..=0x7350E`，码值/说明/短句三面两两不同——码面先立，
//!    后续每一闸的失败才有可归因的出口。
//! 2. **三段限速收口**：先查全三段再统一扣，避免「外段扣了、内段拒绝」的隐性丢配额。
//! 3. **洪泛取证**：`seen == admit + throttle + invalid`，满环覆盖最旧并显性记 `overwritten`。
//! 4. **在途帧丢弃入账**：热拔瞬间剩余帧逐帧计丢弃，引用随帧同生同灭。
//! 5. **热移除 ×10 零残留**：十轮独立建队，终态 `Removed`、队列 0、引用 0。
//! 6. **乱序 ×10 重置**：漏投帧由重置流程冲账，冲账数必须等于滞留数。
//! 7. **账行回读**：固定标签/键序/摘要；追加字符、换标签、改数字三种篡改必被 `audit_row` 判否。
//! 8. **错误路径闭合**：非法迁移、队列溢出、重复序号、零代价/超代价各有确定拒绝值。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49091_register` 登记「补测状态与验收维度」，问“有没有说清楚”；本模块执行「限速与
//!   热移除」，问“错误路径上的账平不平”，账行摘要域 `dev/stage/enq/del/drop` 与其互不复制。
//! - `unx48941_wheel_core` 只提供折叠原语（`FNV_OFF` / `fold_u32` / `fold_byte`）；本模块借摘要域，
//!   不持有轮询语义，也不新增折叠函数。
//! - `unx49089_finalgrep` 做批级复跑留痕；本模块的 ×10 是轮内判据载体，十轮各自新建队列与窗口。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位、窗口与容量（容量/回填单位为帧，轮数为判据载体） ────────
pub const SEG_BASE: u32 = 0x73500;
pub const SEG_SPAN: u32 = 0x40;
pub const STAGES: usize = 3;
pub const CAP_L3: u32 = 64;
pub const CAP_L2: u32 = 32;
pub const CAP_L1: u32 = 16;
pub const REFILL_L3: u32 = 8;
pub const REFILL_L2: u32 = 4;
pub const REFILL_L1: u32 = 2;
pub const BUCKET_CAP: [u32; STAGES] = [CAP_L3, CAP_L2, CAP_L1];
pub const MAX_COST: u32 = 8;
pub const FORENSIC_SLOTS: usize = 16;
pub const HOT_ROUNDS: u32 = 10;
pub const REORDER_ROUNDS: u32 = 10;
pub const QUEUE_SLOTS: usize = 8;
pub const HOT_DEV_BASE: u32 = 0x81;
pub const ROW_FIELDS: usize = 7;
pub const DEF_TAG: &str = "X3D81";
pub const DEF_KEYS: [&str; 5] = ["dev", "stage", "enq", "del", "drop"];
pub const UNKNOWN: &str = "未知 B15 恶意设备防御诊断码";

/// 饱和加法（全模块唯一的计数增长入口）。
pub fn sat_add(v: u32, by: u32) -> u32 {
    v.saturating_add(by)
}

// ── 诊断码 ───────────────────────────────────────────────────────
/// 防御诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Code {
    RateBad,
    StageBad,
    CapBad,
    TokenBad,
    FloodBad,
    RingBad,
    EvidenceBad,
    SeqBad,
    InflightBad,
    DropBad,
    ResidueBad,
    RefBad,
    ReorderBad,
    Incomplete,
}

impl Code {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            Code::RateBad => "限速代价为零或越出 1..=8",
            Code::StageBad => "限速段号越出 0..=2",
            Code::CapBad => "桶容量与 BUCKET_CAP 镜像不符",
            Code::TokenBad => "令牌数越出 0..=容量",
            Code::FloodBad => "洪泛计数不满足 seen = admit + throttle + invalid",
            Code::RingBad => "取证环长度越界或槽位出现空洞",
            Code::EvidenceBad => "证据记录的设备号或代价非法",
            Code::SeqBad => "序号重复或相对期望值倒退",
            Code::InflightBad => "在途队列满或入队失败未计入拒绝数",
            Code::DropBad => "丢弃数不等于入队数减交付数",
            Code::ResidueBad => "热移除后队列仍有残留帧",
            Code::RefBad => "在途帧引用计数未归零",
            Code::ReorderBad => "乱序窗口重置后冲账不等于滞留",
            Code::Incomplete => "防御取证面或账面不完整",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            Code::RateBad => "代价非法：请收敛到 1..=8",
            Code::StageBad => "段号非法：X3 只有三段",
            Code::CapBad => "容量失配：请回读只读镜像",
            Code::TokenBad => "令牌越界：桶不得透支",
            Code::FloodBad => "计数失配：请重算取证面",
            Code::RingBad => "取证环损坏：请重建环",
            Code::EvidenceBad => "证据非法：请核对设备号",
            Code::SeqBad => "序号失序：请检查重传",
            Code::InflightBad => "在途越界：请先收口再入队",
            Code::DropBad => "丢弃未入账：请补记热拔帧",
            Code::ResidueBad => "残留非零：热移除未收口",
            Code::RefBad => "引用泄漏：请核对帧的生死",
            Code::ReorderBad => "重置不全：请重跑乱序轮",
            Code::Incomplete => "取证不全：请补足十轮",
        }
    }
}

/// 全部诊断码（顺序即值序）。
pub const ALL: [Code; 14] = [
    Code::RateBad, Code::StageBad, Code::CapBad, Code::TokenBad,
    Code::FloodBad, Code::RingBad, Code::EvidenceBad, Code::SeqBad,
    Code::InflightBad, Code::DropBad, Code::ResidueBad, Code::RefBad,
    Code::ReorderBad, Code::Incomplete,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => Code::RateBad.detail(),
        2 => Code::StageBad.detail(),
        3 => Code::CapBad.detail(),
        4 => Code::TokenBad.detail(),
        5 => Code::FloodBad.detail(),
        6 => Code::RingBad.detail(),
        7 => Code::EvidenceBad.detail(),
        8 => Code::SeqBad.detail(),
        9 => Code::InflightBad.detail(),
        10 => Code::DropBad.detail(),
        11 => Code::ResidueBad.detail(),
        12 => Code::RefBad.detail(),
        13 => Code::ReorderBad.detail(),
        14 => Code::Incomplete.detail(),
        _ => UNKNOWN,
    }
}

/// 段位自检：14 码值两两不同且全部落在本段窗口内。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ALL.len() {
        let a = ALL[i].val();
        if a <= SEG_BASE || a > SEG_BASE + 0x0F {
            return false;
        }
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[j].val() == a {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 摘要 ─────────────────────────────────────────────────────────
/// 账行摘要：设备号 + 段号 + 入队 + 交付 + 丢弃。
pub fn row_digest(dev: u32, stage: u8, enq: u32, del: u32, drop: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x5833_4438);
    h = p_core::fold_u32(h, dev);
    h = p_core::fold_u32(h, stage as u32);
    h = p_core::fold_u32(h, enq);
    h = p_core::fold_u32(h, del);
    h = p_core::fold_u32(h, drop);
    h
}

/// 确定性混洗（只借折叠原语，不自造摘要域）。
pub fn mix(seed: u32, salt: u32) -> u32 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, seed);
    h = p_core::fold_u32(h, salt);
    h = p_core::fold_byte(h, 0x5A);
    (h ^ (h >> 32)) as u32
}

// ── 账行模型 ─────────────────────────────────────────────────────
/// 一行热移除账：设备号 + 段号 + 入队 + 交付 + 丢弃。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DefRow {
    pub dev: u32,
    pub stage: u8,
    pub enq: u32,
    pub del: u32,
    pub drop: u32,
}

impl DefRow {
    /// 账平：交付不大于入队，丢弃恰为差额。
    pub fn balanced(&self) -> bool {
        self.del <= self.enq && self.drop == self.enq - self.del
    }
}

/// 解析十进制（拒绝空串、非数字、溢出）。
pub fn parse_u32(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?;
        acc = acc.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

/// 解析 16 位十六进制摘要（长度必须恰为 16）。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut acc = 0u64;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let v = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        acc = acc.checked_mul(16)?;
        acc = acc.checked_add(v)?;
        i += 1;
    }
    Some(acc)
}

/// 按固定标签与键序拆行；返回各键值与摘要值。
pub fn row_fields<'a>(row: &'a str, tag: &str, keys: &[&str]) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = row.split('|').collect();
    if parts.len() != keys.len() + 2 || parts[0] != tag {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < keys.len() {
        let piece = parts[i + 1];
        let rest = piece.strip_prefix(keys[i])?;
        let value = rest.strip_prefix('=')?;
        if value.is_empty() {
            return None;
        }
        out.push(value);
        i += 1;
    }
    let d = parts[keys.len() + 1].strip_prefix("digest=")?;
    if d.len() != 16 {
        return None;
    }
    out.push(d);
    Some(out)
}

/// 取第 i 个字段并解析为十进制。
fn field_u32(f: &[&str], i: usize) -> Option<u32> {
    f.get(i).and_then(|s| parse_u32(s))
}

/// 渲染账行：`X3D81|dev=|stage=|enq=|del=|drop=|digest=`。
pub fn render_row(r: &DefRow) -> String {
    format!(
        "{}|dev={}|stage={}|enq={}|del={}|drop={}|digest={:016x}",
        DEF_TAG, r.dev, r.stage, r.enq, r.del, r.drop,
        row_digest(r.dev, r.stage, r.enq, r.del, r.drop)
    )
}

/// 账行回读：键序、账平、区间、摘要四重校验。
pub fn audit_row(row: &str) -> bool {
    let f = match row_fields(row, DEF_TAG, &DEF_KEYS) {
        Some(v) if v.len() == DEF_KEYS.len() + 1 => v,
        _ => return false,
    };
    let dev = match field_u32(&f, 0) {
        Some(v) if v >= HOT_DEV_BASE => v,
        _ => return false,
    };
    let stage = match field_u32(&f, 1) {
        Some(v) if v >= 1 && v <= STAGES as u32 => v,
        _ => return false,
    };
    let enq = match field_u32(&f, 2) {
        Some(v) => v,
        None => return false,
    };
    let del = match field_u32(&f, 3) {
        Some(v) if v <= enq => v,
        _ => return false,
    };
    let drop = match field_u32(&f, 4) {
        Some(v) => v,
        None => return false,
    };
    let row = DefRow { dev, stage: stage as u8, enq, del, drop };
    if !row.balanced() {
        return false;
    }
    let want = match f.get(5).and_then(|s| parse_hex16(s)) {
        Some(v) => v,
        None => return false,
    };
    row_digest(row.dev, row.stage, row.enq, row.del, row.drop) == want
}

/// 十行热移除账（每轮一行，设备号唯一）。
pub fn export_rows() -> Vec<String> {
    let reps = hot_remove_x10();
    let mut out = Vec::new();
    for r in reps.iter() {
        out.push(render_row(&DefRow {
            dev: HOT_DEV_BASE + r.round,
            stage: (1 + (r.round % STAGES as u32)) as u8,
            enq: r.enq,
            del: r.del,
            drop: r.drop,
        }));
    }
    out
}

/// 十行两两不同且逐行可回读。
pub fn rows_stable() -> bool {
    let rows = export_rows();
    if rows.len() != HOT_ROUNDS as usize {
        return false;
    }
    for (i, row) in rows.iter().enumerate() {
        if !audit_row(row) {
            return false;
        }
        if rows.iter().skip(i + 1).any(|other| other == row) {
            return false;
        }
    }
    true
}

// ── X3 三段限速 ──────────────────────────────────────────────────
/// 放行判决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 三段全过并已扣令牌。
    Admit,
    /// 至少一段令牌不足。
    Throttle,
    /// 代价非法（零或越上限）。
    Invalid,
}

/// 单段令牌桶：只做减法，回填只能走 `tick`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bucket {
    cap: u32,
    tokens: u32,
    refill: u32,
}

impl Bucket {
    /// 满桶起手。
    pub const fn new(cap: u32, refill: u32) -> Bucket {
        Bucket { cap, tokens: cap, refill }
    }
    /// 容量。
    pub fn cap(&self) -> u32 { self.cap }
    /// 当前令牌。
    pub fn tokens(&self) -> u32 { self.tokens }
    /// 单 tick 回填额。
    pub fn refill(&self) -> u32 { self.refill }
    /// 扣令牌（不足原样返回 false，不透支）。
    pub fn take(&mut self, cost: u32) -> bool {
        if cost == 0 || cost > self.tokens {
            return false;
        }
        self.tokens -= cost;
        true
    }
    /// 按 tick 回填（饱和到容量）。
    pub fn tick(&mut self) {
        let room = self.cap.saturating_sub(self.tokens);
        self.tokens = sat_add(self.tokens, self.refill.min(room));
    }
    /// 复位到指定令牌数（饱和到容量；复跑专用）。
    pub fn install(&mut self, n: u32) -> bool {
        self.tokens = if n > self.cap { self.cap } else { n };
        true
    }
}

/// X3 三段限速执行器：先查全三段，再统一扣。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Governor {
    buckets: [Bucket; STAGES],
    pub grants: u32,
    pub throttles: u32,
    pub invalid: u32,
    pub attempts: u32,
    pub ticks: u32,
}

impl Governor {
    /// 满桶起手。
    pub const fn new() -> Governor {
        Governor {
            buckets: [Bucket::new(CAP_L3, REFILL_L3), Bucket::new(CAP_L2, REFILL_L2), Bucket::new(CAP_L1, REFILL_L1)],
            grants: 0, throttles: 0, invalid: 0, attempts: 0, ticks: 0,
        }
    }

    /// 段令牌数。
    pub fn tokens(&self, stage: usize) -> u32 {
        match self.buckets.get(stage) { Some(b) => b.tokens(), None => 0 }
    }

    /// 段容量。
    pub fn cap(&self, stage: usize) -> u32 {
        match self.buckets.get(stage) { Some(b) => b.cap(), None => 0 }
    }

    /// 段回填额。
    pub fn refill(&self, stage: usize) -> u32 {
        match self.buckets.get(stage) { Some(b) => b.refill(), None => 0 }
    }

    /// 复位某段令牌（越段返回 false，不 panic）。
    pub fn fill(&mut self, stage: usize, n: u32) -> bool {
        match self.buckets.get_mut(stage) { Some(b) => b.install(n), None => false }
    }

    /// 单 tick 回填全段。
    pub fn tick(&mut self) {
        let mut i = 0usize;
        while i < STAGES {
            if let Some(b) = self.buckets.get_mut(i) {
                b.tick();
            }
            i += 1;
        }
        self.ticks = sat_add(self.ticks, 1);
    }

    /// 三段全过才扣：避免「外段扣了、内段拒绝」的隐性丢配额。
    pub fn admit(&mut self, cost: u32) -> Verdict {
        self.attempts = sat_add(self.attempts, 1);
        if cost == 0 || cost > MAX_COST {
            self.invalid = sat_add(self.invalid, 1);
            return Verdict::Invalid;
        }
        let mut i = 0usize;
        while i < STAGES {
            match self.buckets.get(i) {
                Some(b) if b.tokens() >= cost => {}
                _ => {
                    self.throttles = sat_add(self.throttles, 1);
                    return Verdict::Throttle;
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < STAGES {
            if let Some(b) = self.buckets.get_mut(j) {
                b.take(cost);
            }
            j += 1;
        }
        self.grants = sat_add(self.grants, 1);
        Verdict::Admit
    }

    /// 令牌始终不越容量，且容量与只读镜像一致。
    pub fn residue_ok(&self) -> bool {
        let mut i = 0usize;
        while i < STAGES {
            match self.buckets.get(i) {
                Some(b) => {
                    if b.tokens() > b.cap() || Some(b.cap()) != BUCKET_CAP.get(i).copied() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 三段令牌合计。
    pub fn total_tokens(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < STAGES {
            n = sat_add(n, self.tokens(i));
            i += 1;
        }
        n
    }

    /// 判决计数三分法可加。
    pub fn partition_ok(&self) -> bool {
        sat_add(sat_add(self.grants, self.throttles), self.invalid) == self.attempts
    }
}

// ── 洪泛取证面 ───────────────────────────────────────────────────
/// 洪泛计数面（可加性自洽）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FloodCounters {
    pub seen: u32,
    pub admitted: u32,
    pub throttled: u32,
    pub invalid: u32,
    pub drop: u32,
    pub max_cost: u32,
}

impl FloodCounters {
    /// 全零起手。
    pub const fn new() -> FloodCounters {
        FloodCounters { seen: 0, admitted: 0, throttled: 0, invalid: 0, drop: 0, max_cost: 0 }
    }

    /// 记一次观测（计数全部饱和增长，不溢出）。
    pub fn observe(&mut self, v: Verdict, cost: u32) -> Verdict {
        self.seen = sat_add(self.seen, 1);
        match v {
            Verdict::Admit => self.admitted = sat_add(self.admitted, 1),
            Verdict::Throttle => {
                self.throttled = sat_add(self.throttled, 1);
                self.drop = sat_add(self.drop, cost);
            }
            Verdict::Invalid => self.invalid = sat_add(self.invalid, 1),
        }
        if cost > self.max_cost {
            self.max_cost = cost;
        }
        v
    }

    /// 可加性：观测数恰为三格之和。
    pub fn partition_ok(&self) -> bool {
        sat_add(sat_add(self.admitted, self.throttled), self.invalid) == self.seen
    }
}

/// 单条取证记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Evidence {
    pub dev: u32,
    pub seq: u32,
    pub cost: u32,
    pub verdict: Verdict,
}

impl Evidence {
    /// 记录合法：设备号在窗口内、代价在 1..=8、序号非哨兵。
    pub fn valid(&self) -> bool {
        self.dev >= HOT_DEV_BASE && self.cost >= 1 && self.cost <= MAX_COST && self.seq != u32::MAX
    }
}

/// 定容取证环：满环后覆盖最旧并显性记 `overwritten`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EvidenceRing {
    slots: [Option<Evidence>; FORENSIC_SLOTS],
    head: usize,
    len: usize,
    /// 覆盖次数（证据不静默消失）。
    pub overwritten: u32,
}

impl EvidenceRing {
    /// 空环。
    pub const fn new() -> EvidenceRing {
        EvidenceRing { slots: [None; FORENSIC_SLOTS], head: 0, len: 0, overwritten: 0 }
    }

    /// 压入一条证据；满环时覆盖最旧并返回 false（仍已记账）。
    pub fn push(&mut self, ev: Evidence) -> bool {
        if self.len < FORENSIC_SLOTS {
            let at = (self.head + self.len) % FORENSIC_SLOTS;
            match self.slots.get_mut(at) {
                Some(slot) => {
                    *slot = Some(ev);
                    self.len += 1;
                    true
                }
                None => false,
            }
        } else {
            let at = self.head;
            if let Some(slot) = self.slots.get_mut(at) {
                *slot = Some(ev);
            }
            self.head = (self.head + 1) % FORENSIC_SLOTS;
            self.overwritten = sat_add(self.overwritten, 1);
            false
        }
    }

    /// 当前证据数。
    pub fn len(&self) -> usize { self.len }

    /// 是否为空。
    pub fn is_empty(&self) -> bool { self.len == 0 }

    /// 按旧→新序取第 i 条。
    pub fn get(&self, i: usize) -> Option<&Evidence> {
        if i >= self.len {
            return None;
        }
        let at = (self.head + i) % FORENSIC_SLOTS;
        self.slots.get(at).and_then(|s| s.as_ref())
    }

    /// 最新一条。
    pub fn newest(&self) -> Option<&Evidence> {
        if self.len == 0 { return None; }
        self.get(self.len - 1)
    }

    /// 槽位无空洞（前 len 个位置必须都有记录）。
    pub fn coherent(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if self.get(i).is_none() {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 热移除错误路径 ───────────────────────────────────────────────
/// 热移除状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HotState {
    /// 在位。
    Present,
    /// 排空在途帧。
    Draining,
    /// 已移除终态。
    Removed,
    /// 重置流程中。
    Resetting,
    /// 复位后上线。
    Live,
}

impl HotState {
    /// 状态取值。
    pub fn tag(self) -> &'static str {
        match self {
            HotState::Present => "present",
            HotState::Draining => "draining",
            HotState::Removed => "removed",
            HotState::Resetting => "resetting",
            HotState::Live => "live",
        }
    }
}

/// 推进一步：合法则迁移并计数，非法则记拒绝（错误路径不静默）。
pub fn advance(st: &mut HotState, to: HotState, transitions: &mut u32, refused: &mut u32) {
    if transition_allowed(*st, to) {
        *st = to;
        *transitions = sat_add(*transitions, 1);
    } else {
        *refused = sat_add(*refused, 1);
    }
}

/// 迁移合法性：只放行错误路径需要的七条边，其余一律拒绝。
pub fn transition_allowed(from: HotState, to: HotState) -> bool {
    match (from, to) {
        (HotState::Present, HotState::Draining) => true,
        (HotState::Present, HotState::Removed) => true,
        (HotState::Draining, HotState::Removed) => true,
        (HotState::Removed, HotState::Resetting) => true,
        (HotState::Resetting, HotState::Removed) => true,
        (HotState::Resetting, HotState::Live) => true,
        (HotState::Live, HotState::Present) => true,
        _ => false,
    }
}

/// 在途帧。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frame {
    pub dev: u32,
    pub seq: u32,
    pub cost: u32,
}

/// 在途队列：帧与引用计数同生同灭。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Inflight {
    slots: [Option<Frame>; QUEUE_SLOTS],
    head: usize,
    len: usize,
    refs: u32,
}

impl Inflight {
    /// 空队。
    pub const fn new() -> Inflight {
        Inflight { slots: [None; QUEUE_SLOTS], head: 0, len: 0, refs: 0 }
    }

    /// 在途帧数。
    pub fn len(&self) -> usize { self.len }
    /// 是否为空。
    pub fn is_empty(&self) -> bool { self.len == 0 }
    /// 引用计数。
    pub fn refs(&self) -> u32 { self.refs }

    /// 入队：满队返回 false（由上层计入拒绝，不静默）。
    pub fn enqueue(&mut self, f: Frame) -> bool {
        if self.len >= QUEUE_SLOTS {
            return false;
        }
        let at = (self.head + self.len) % QUEUE_SLOTS;
        match self.slots.get_mut(at) {
            Some(slot) => {
                *slot = Some(f);
                self.len += 1;
                self.refs = sat_add(self.refs, 1);
                true
            }
            None => false,
        }
    }

    /// 出队（交付）：引用随帧释放。
    pub fn pop(&mut self) -> Option<Frame> {
        if self.len == 0 {
            return None;
        }
        let at = self.head;
        let out = self.slots.get_mut(at).and_then(|s| s.take());
        if out.is_some() {
            self.head = (self.head + 1) % QUEUE_SLOTS;
            self.len -= 1;
            self.refs = self.refs.saturating_sub(1);
        }
        out
    }

    /// 热拔收口：逐帧计丢弃，队头/长度/引用全部归零。
    pub fn drop_all(&mut self) -> u32 {
        let mut n = 0u32;
        while self.pop().is_some() {
            n = sat_add(n, 1);
        }
        self.head = 0;
        self.len = 0;
        self.refs = 0;
        n
    }

    /// 队列残留帧数。
    pub fn residue(&self) -> usize { self.len }

    /// 引用泄漏（引用数多于在队帧的部分）。
    pub fn leaked(&self) -> u32 {
        self.refs.saturating_sub(self.len as u32)
    }
}

/// 单轮热移除报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HotReport {
    pub round: u32,
    pub state: HotState,
    pub enq: u32,
    pub del: u32,
    pub drop: u32,
    pub residue: u32,
    pub leaked: u32,
    pub transitions: u32,
    pub refused: u32,
}

impl HotReport {
    /// 丢弃入账：丢弃恰为入队减交付。
    pub fn accounted(&self) -> bool {
        self.del <= self.enq && self.drop == self.enq.saturating_sub(self.del)
    }

    /// 终验零残留：账平 + 队列 0 + 引用 0 + 终态 Removed + 无非法迁移。
    pub fn clean(&self) -> bool {
        self.accounted()
            && self.residue == 0
            && self.leaked == 0
            && self.state == HotState::Removed
            && self.refused == 0
            && self.transitions == 4
    }
}

/// 单轮热移除：建队 → 排空 → 热拔收口 → 重置 → 回终态。
pub fn hot_remove_round(round: u32) -> HotReport {
    let dev = HOT_DEV_BASE + round;
    let seed = mix(dev, round);
    let total = 4 + (seed % 5) as u32;
    let mut q = Inflight::new();
    let mut st = HotState::Present;
    let mut transitions = 0u32;
    let mut refused = 0u32;
    let mut enq = 0u32;

    let mut i = 0u32;
    while i < total {
        if q.enqueue(Frame { dev, seq: i, cost: 1 + (mix(i, seed) % MAX_COST) }) {
            enq = sat_add(enq, 1);
        } else {
            refused = sat_add(refused, 1);
        }
        i += 1;
    }

    // 在位 → 排空：先停再交付残余帧。
    advance(&mut st, HotState::Draining, &mut transitions, &mut refused);

    let mut del = 0u32;
    let target = seed % 4;
    let mut k = 0u32;
    while k < target {
        if q.pop().is_some() {
            del = sat_add(del, 1);
        }
        k += 1;
    }

    // 排空 → 已移除：热拔瞬间在途帧全部入账为丢弃。
    advance(&mut st, HotState::Removed, &mut transitions, &mut refused);
    let drop = q.drop_all();

    // 已移除 → 重置 → 已移除：重置必须回到同一终态且不留残留。
    advance(&mut st, HotState::Resetting, &mut transitions, &mut refused);
    advance(&mut st, HotState::Removed, &mut transitions, &mut refused);

    HotReport {
        round, state: st, enq, del, drop,
        residue: q.residue() as u32, leaked: q.leaked(), transitions, refused,
    }
}

/// 热移除 ×10（判据载体：十轮独立建队，不复用槽位）。
pub fn hot_remove_x10() -> Vec<HotReport> {
    let mut out = Vec::new();
    let mut i = 0u32;
    while i < HOT_ROUNDS {
        out.push(hot_remove_round(i + 1));
        i += 1;
    }
    out
}

/// 十轮是否全部零残留。
pub fn hot_remove_clean() -> bool {
    let reps = hot_remove_x10();
    reps.len() == HOT_ROUNDS as usize && reps.iter().all(|r| r.clean())
}

/// 十轮丢弃合计。
pub fn hot_drop_total() -> u32 {
    let mut n = 0u32;
    for r in hot_remove_x10().iter() {
        n = sat_add(n, r.drop);
    }
    n
}

// ── 乱序重置流程 ─────────────────────────────────────────────────
/// 计划序号（固定乱序表，0..=7 各一次）。
pub const SEQ_PLAN: [u32; 8] = [3, 1, 5, 0, 2, 7, 4, 6];

/// 投递结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivery {
    /// 正序交付。
    InOrder,
    /// 挂起待前序。
    Held,
    /// 重复序号。
    Duplicate,
    /// 窗口已满拒收。
    Refused,
}

/// 乱序窗口：挂起重排 + 重置冲账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReorderWindow {
    expect: u32,
    held: [Option<u32>; QUEUE_SLOTS],
    held_len: usize,
    pub delivered: u32,
    pub held_total: u32,
    pub duplicates: u32,
    pub resets: u32,
    pub flushed: u32,
}

impl ReorderWindow {
    /// 空窗。
    pub const fn new() -> ReorderWindow {
        ReorderWindow {
            expect: 0, held: [None; QUEUE_SLOTS], held_len: 0,
            delivered: 0, held_total: 0, duplicates: 0, resets: 0, flushed: 0,
        }
    }

    /// 当前期望序号。
    pub fn expect(&self) -> u32 { self.expect }
    /// 挂起数。
    pub fn held_len(&self) -> usize { self.held_len }

    /// 观察一个序号：正序交付、挂起、重复或拒收。
    pub fn observe(&mut self, seq: u32) -> Delivery {
        if seq < self.expect {
            self.duplicates = sat_add(self.duplicates, 1);
            return Delivery::Duplicate;
        }
        if seq == self.expect {
            self.delivered = sat_add(self.delivered, 1);
            self.expect = sat_add(self.expect, 1);
            self.drain_ready();
            return Delivery::InOrder;
        }
        let mut i = 0usize;
        while i < QUEUE_SLOTS {
            if self.held.get(i).and_then(|x| *x) == Some(seq) {
                self.duplicates = sat_add(self.duplicates, 1);
                return Delivery::Duplicate;
            }
            i += 1;
        }
        if self.held_len >= QUEUE_SLOTS {
            return Delivery::Refused;
        }
        let mut at = 0usize;
        while at < QUEUE_SLOTS {
            if self.held.get(at).and_then(|x| *x).is_none() {
                match self.held.get_mut(at) {
                    Some(slot) => {
                        *slot = Some(seq);
                        self.held_len += 1;
                        self.held_total = sat_add(self.held_total, 1);
                        return Delivery::Held;
                    }
                    None => return Delivery::Refused,
                }
            }
            at += 1;
        }
        Delivery::Refused
    }

    /// 前序到位后连锁释放（扫全部槽位，空槽即已释放，无需压实）。
    fn drain_ready(&mut self) {
        let mut progressed = true;
        while progressed {
            progressed = false;
            let mut i = 0usize;
            while i < QUEUE_SLOTS {
                if self.held.get(i).and_then(|x| *x) == Some(self.expect) {
                    if let Some(slot) = self.held.get_mut(i) {
                        *slot = None;
                    }
                    self.delivered = sat_add(self.delivered, 1);
                    self.expect = sat_add(self.expect, 1);
                    self.held_len = self.held_len.saturating_sub(1);
                    progressed = true;
                }
                i += 1;
            }
        }
    }

    /// 重置流程：冲账滞留帧、清零期望、记一次重置。
    pub fn reset_flow(&mut self) -> u32 {
        let flushed = self.held_len as u32;
        let mut i = 0usize;
        while i < QUEUE_SLOTS {
            if let Some(slot) = self.held.get_mut(i) {
                *slot = None;
            }
            i += 1;
        }
        self.held_len = 0;
        self.flushed = sat_add(self.flushed, flushed);
        self.expect = 0;
        self.resets = sat_add(self.resets, 1);
        flushed
    }
}

/// 单轮乱序报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReorderReport {
    pub round: u32,
    pub planned: u32,
    pub delivered: u32,
    pub flushed: u32,
    pub residue: u32,
    pub refused: u32,
    pub duplicates: u32,
    pub resets: u32,
    pub held: u32,
}

impl ReorderReport {
    /// 每帧有下落：交付 + 冲账 == 计划。
    pub fn accounted(&self) -> bool {
        self.delivered.saturating_add(self.flushed) == self.planned
    }

    /// 重置流程全过：有下落 + 残留 0 + 恰好一次重置。
    pub fn clean(&self) -> bool {
        self.accounted() && self.residue == 0 && self.resets == 1 && self.refused == 0
    }
}

/// 单轮乱序：乱序投递 → 重置 → 冲账核对。
pub fn reorder_round(round: u32) -> ReorderReport {
    let rot = (mix(HOT_DEV_BASE, round) % SEQ_PLAN.len() as u32) as usize;
    // 偶数轮刻意漏投 6 号帧（错误路径），由重置流程冲账。
    let gap = (round % 2) == 0;
    let mut w = ReorderWindow::new();
    let mut planned = 0u32;
    let mut refused = 0u32;
    let mut i = 0usize;
    while i < SEQ_PLAN.len() {
        let idx = (rot + i) % SEQ_PLAN.len();
        let seq = match SEQ_PLAN.get(idx) { Some(v) => *v, None => 0 };
        if gap && seq == 6 {
            i += 1;
            continue;
        }
        planned = sat_add(planned, 1);
        if w.observe(seq) == Delivery::Refused {
            refused = sat_add(refused, 1);
        }
        i += 1;
    }
    w.reset_flow();
    ReorderReport {
        round, planned, delivered: w.delivered, flushed: w.flushed,
        residue: w.held_len() as u32, refused,
        duplicates: w.duplicates, resets: w.resets, held: w.held_total,
    }
}

/// 乱序 ×10（判据载体：十轮各自新建窗口）。
pub fn reorder_x10() -> Vec<ReorderReport> {
    let mut out = Vec::new();
    let mut i = 0u32;
    while i < REORDER_ROUNDS {
        out.push(reorder_round(i + 1));
        i += 1;
    }
    out
}

/// 十轮重置流程是否全过。
pub fn reorder_clean() -> bool {
    let reps = reorder_x10();
    reps.len() == REORDER_ROUNDS as usize && reps.iter().all(|r| r.clean())
}

// ── 八项判据 ─────────────────────────────────────────────────────
/// 八项自检（真算，不预置结论）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1) 三段限速收口：满桶连打 100 次，只应放过最内段容量那么多。
    let mut g = Governor::new();
    let mut grants = 0u32;
    let mut throttles = 0u32;
    let mut i = 0usize;
    while i < 100 {
        match g.admit(1) {
            Verdict::Admit => grants = sat_add(grants, 1),
            _ => throttles = sat_add(throttles, 1),
        }
        i += 1;
    }
    let bounded = g.tokens(0) <= g.cap(0) && g.tokens(1) <= g.cap(1) && g.tokens(2) <= g.cap(2);
    let drained = g.tokens(2) == 0;
    let before = g.total_tokens();
    g.tick();
    let want_total = sat_add(sat_add(before, REFILL_L3), sat_add(REFILL_L2, REFILL_L1));
    let refilled = g.tokens(2) == g.refill(2) && g.total_tokens() == want_total;
    let zero_cost = g.admit(0) == Verdict::Invalid;
    let over_cost = g.admit(MAX_COST + 1) == Verdict::Invalid;
    let ok = grants == CAP_L1
        && throttles == 100 - CAP_L1
        && bounded
        && drained
        && refilled
        && zero_cost
        && over_cost
        && g.invalid == 2
        && g.partition_ok()
        && g.residue_ok();
    out.push(("F49081/三段限速收口", ok, format!("grants={} throttle={} tokens={}/{}/{} cap={}/{}/{} refill={} invalid={} bounded={}", grants, throttles, g.tokens(0), g.tokens(1), g.tokens(2), g.cap(0), g.cap(1), g.cap(2), refilled, g.invalid, bounded)));

    // 2) 洪泛取证：计数可加、环满覆盖、证据合法、饱和加法。
    let mut g = Governor::new();
    let mut c = FloodCounters::new();
    let mut ring = EvidenceRing::new();
    let mut peak = 0u32;
    let mut i = 0u32;
    while i < 50 {
        let cost = 1 + (mix(HOT_DEV_BASE, i) % MAX_COST);
        let v = g.admit(cost);
        c.observe(v, cost);
        ring.push(Evidence { dev: HOT_DEV_BASE, seq: i, cost, verdict: v });
        if cost > peak {
            peak = cost;
        }
        i += 1;
    }
    let newest_ok = match ring.newest() {
        Some(e) => e.seq == 49 && e.valid(),
        None => false,
    };
    let sat_ok = sat_add(u32::MAX, 7) == u32::MAX && sat_add(u32::MAX - 1, 5) == u32::MAX;
    let ok = c.seen == 50
        && c.partition_ok()
        && c.throttled > 0
        && c.max_cost == peak
        && ring.len() == FORENSIC_SLOTS
        && ring.overwritten == 50 - FORENSIC_SLOTS as u32
        && ring.coherent()
        && newest_ok
        && sat_ok
        && g.partition_ok();
    out.push(("F49081/洪泛取证计数", ok, format!("seen={} admit={} throttle={} invalid={} peak={} ring={} overwritten={} newest={} sat={}", c.seen, c.admitted, c.throttled, c.invalid, c.max_cost, ring.len(), ring.overwritten, newest_ok, sat_ok)));

    // 3) 账行回读：十行唯一、逐行可回读、三种篡改必拒。
    let rows = export_rows();
    let tamper_tail = match rows.first() {
        Some(r) => !audit_row(&format!("{}x", r)),
        None => false,
    };
    let tamper_tag = match rows.first() {
        Some(r) => !audit_row(&r.replace(DEF_TAG, "X3D99")),
        None => false,
    };
    let tamper_drop = !audit_row(&render_row(&DefRow {
        dev: HOT_DEV_BASE, stage: 1, enq: 9, del: 1, drop: 7,
    }));
    let ok = rows.len() == HOT_ROUNDS as usize && ROW_FIELDS == 7
        && rows_stable() && tamper_tail && tamper_tag && tamper_drop;
    out.push(("F49081/账行回读与篡改拒绝", ok, format!("rows={} fields={} stable={} tail={} tag={} drop={}", rows.len(), ROW_FIELDS, rows_stable(), tamper_tail, tamper_tag, tamper_drop)));

    // 4) 段位与码面：14 码两两不同，说明与短句互不共用，段外落 UNKNOWN。
    let mut d_uniq = true;
    let mut m_uniq = true;
    for (i, a) in ALL.iter().enumerate() {
        for b in ALL.iter().skip(i + 1) {
            d_uniq &= a.detail() != b.detail();
            m_uniq &= a.message() != b.message();
        }
    }
    let first_ok = detail(SEG_BASE + 1) == Code::RateBad.detail();
    let out_lo = detail(SEG_BASE) == UNKNOWN;
    let out_hi = detail(SEG_BASE + 15) == UNKNOWN;
    let out_zero = detail(0) == UNKNOWN;
    let ok = segment_guard() && ALL.len() == 14 && d_uniq && m_uniq
        && first_ok && out_lo && out_hi && out_zero
        && SEG_SPAN == 0x40 && SEG_BASE + SEG_SPAN > SEG_BASE;
    out.push(("F49081/段位与码面", ok, format!("codes={} guard={} d_uniq={} m_uniq={} first={} out={}/{}/{} span={:#x}", ALL.len(), segment_guard(), d_uniq, m_uniq, first_ok, out_lo, out_hi, out_zero, SEG_SPAN)));

    // 5) 热移除 ×10：十轮终态 Removed 且零残留。
    let reps = hot_remove_x10();
    let mut all_clean = true;
    let mut index_ok = true;
    for (i, r) in reps.iter().enumerate() {
        all_clean &= r.clean();
        index_ok &= r.round == i as u32 + 1 && r.state == HotState::Removed;
    }
    let ok = reps.len() == HOT_ROUNDS as usize && all_clean && index_ok && hot_remove_clean();
    out.push(("F49082/热移除×10零残留", ok, format!("rounds={} clean={} index_ok={} drops={} state={}", reps.len(), all_clean, index_ok, hot_drop_total(), HotState::Removed.tag())));

    // 6) 在途帧丢弃入账：每轮 drop == enq − del，合计大于零且与总账一致。
    let reps = hot_remove_x10();
    let mut accounted = 0u32;
    let mut enq_sum = 0u32;
    let mut del_sum = 0u32;
    let mut drop_sum = 0u32;
    for r in reps.iter() {
        if r.accounted() && r.drop == r.enq.saturating_sub(r.del) {
            accounted = sat_add(accounted, 1);
        }
        enq_sum = sat_add(enq_sum, r.enq);
        del_sum = sat_add(del_sum, r.del);
        drop_sum = sat_add(drop_sum, r.drop);
    }
    let ok = accounted == HOT_ROUNDS
        && drop_sum > 0
        && drop_sum == hot_drop_total()
        && enq_sum == del_sum.saturating_add(drop_sum);
    out.push(("F49082/在途帧丢弃入账", ok, format!("accounted={} enq={} del={} drop={} total={}", accounted, enq_sum, del_sum, drop_sum, hot_drop_total())));

    // 7) 乱序 ×10 重置：每轮交付 + 冲账 == 计划，重置恰好一次。
    let reps = reorder_x10();
    let mut all_clean = true;
    let mut accounted = 0u32;
    let mut flushed = 0u32;
    for r in reps.iter() {
        all_clean &= r.clean();
        if r.accounted() {
            accounted = sat_add(accounted, 1);
        }
        flushed = sat_add(flushed, r.flushed);
    }
    let ok = reps.len() == REORDER_ROUNDS as usize && all_clean
        && accounted == REORDER_ROUNDS && flushed > 0 && reorder_clean();
    out.push(("F49082/乱序×10重置流程", ok, format!("rounds={} clean={} accounted={} flushed={}", reps.len(), all_clean, accounted, flushed)));

    // 8) 错误路径闭合：非法迁移、队列溢出、重复序号、非法代价各有确定拒绝值。
    let bad_edges = u32::from(transition_allowed(HotState::Present, HotState::Live))
        + u32::from(transition_allowed(HotState::Removed, HotState::Draining))
        + u32::from(transition_allowed(HotState::Live, HotState::Removed));
    let edges_ok = bad_edges == 0
        && transition_allowed(HotState::Present, HotState::Draining)
        && transition_allowed(HotState::Draining, HotState::Removed)
        && transition_allowed(HotState::Removed, HotState::Resetting)
        && transition_allowed(HotState::Resetting, HotState::Removed);
    let mut q = Inflight::new();
    let mut i = 0usize;
    while i < QUEUE_SLOTS + 2 {
        q.enqueue(Frame { dev: HOT_DEV_BASE, seq: i as u32, cost: 1 });
        i += 1;
    }
    let over_ok = q.len() == QUEUE_SLOTS && q.refs() as usize == QUEUE_SLOTS && q.leaked() == 0;
    let drained = q.drop_all();
    let drain_ok = drained == QUEUE_SLOTS as u32 && q.is_empty() && q.refs() == 0
        && q.leaked() == 0 && q.pop().is_none() && q.drop_all() == 0;
    let mut g = Governor::new();
    let cost_ok = g.admit(0) == Verdict::Invalid
        && g.admit(MAX_COST + 1) == Verdict::Invalid
        && g.admit(MAX_COST) == Verdict::Admit
        && g.invalid == 2
        && g.fill(STAGES, 1) == false;
    let mut w = ReorderWindow::new();
    let dup_ok = w.observe(2) == Delivery::Held
        && w.observe(2) == Delivery::Duplicate
        && w.observe(0) == Delivery::InOrder
        && w.delivered == 1
        && w.observe(1) == Delivery::InOrder
        && w.delivered == 3
        && w.held_len() == 0;
    let ring_ok = {
        let mut r = EvidenceRing::new();
        let mut i = 0u32;
        while i < 20 {
            r.push(Evidence { dev: HOT_DEV_BASE, seq: i, cost: 1, verdict: Verdict::Admit });
            i += 1;
        }
        let newest = r.get(FORENSIC_SLOTS - 1).map(|e| e.seq);
        !r.is_empty() && r.len() == FORENSIC_SLOTS && r.overwritten == 4
            && r.coherent() && newest == Some(19)
    };
    let ok = edges_ok && over_ok && drain_ok && cost_ok && dup_ok && ring_ok;
    out.push(("F49082/错误路径闭合", ok, format!("edges={} bad={} overflow={} drain={} cost={} dup={} ring={}", edges_ok, bad_edges, over_ok, drain_ok, cost_ok, dup_ok, ring_ok)));

    out
}

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49081_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49081 恶意设备防御总成");
    for (name, ok, detail) in extra_checks().iter() {
        s.add_dyn(*name, *ok, detail);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail_unique() -> bool {
        let mut i = 0usize;
        while i < ALL.len() {
            let mut j = i + 1;
            while j < ALL.len() {
                if ALL[i].detail() == ALL[j].detail() { return false; }
                j += 1;
            }
            i += 1;
        }
        true
    }

    fn message_unique() -> bool {
        let mut i = 0usize;
        while i < ALL.len() {
            let mut j = i + 1;
            while j < ALL.len() {
                if ALL[i].message() == ALL[j].message() { return false; }
                j += 1;
            }
            i += 1;
        }
        true
    }

    fn check_names() -> Vec<&'static str> {
        extra_checks().iter().map(|c| c.0).collect()
    }

    #[test]
    fn code_values_monotonic_in_window() {
        for (i, c) in ALL.iter().enumerate() { assert_eq!(c.val(), SEG_BASE + 1 + i as u32); }
        assert_eq!(ALL.len(), 14);
        assert!(segment_guard());
    }

    #[test]
    fn code_details_unique() { assert!(detail_unique()); }

    #[test]
    fn code_messages_unique() { assert!(message_unique()); }

    #[test]
    fn code_detail_differs_from_message() {
        for c in ALL { assert_ne!(c.detail(), c.message()); }
    }

    #[test]
    fn free_detail_matches_enum() {
        for c in ALL { assert_eq!(detail(c.val()), c.detail()); }
    }

    #[test]
    fn free_detail_outside_segment_is_unknown() {
        for c in [0u32, SEG_BASE, SEG_BASE + 0x0F, SEG_BASE + 0x40, u32::MAX] { assert_eq!(detail(c), UNKNOWN); }
    }

    #[test]
    fn segment_window_is_base_plus_span() {
        assert_eq!((SEG_BASE, SEG_SPAN), (0x73500, 0x40));
        assert_eq!(ALL[13].val(), SEG_BASE + 0x0E);
    }

    #[test]
    fn checks_are_exactly_eight() { assert_eq!(extra_checks().len(), 8); }

    #[test]
    fn checks_names_unique() {
        let names = check_names();
        for (i, n) in names.iter().enumerate() {
            for m in names.iter().skip(i + 1) { assert_ne!(n, m); }
        }
    }

    #[test]
    fn checks_are_four_plus_four() {
        let names = check_names();
        assert_eq!(names.iter().filter(|n| n.starts_with("F49081/")).count(), 4);
        assert_eq!(names.iter().filter(|n| n.starts_with("F49082/")).count(), 4);
    }

    #[test]
    fn checks_all_green() {
        for c in &extra_checks() { assert!(c.1, "check {} red: {}", c.0, c.2); }
    }

    #[test]
    fn checks_details_nonempty() {
        for c in &extra_checks() { assert!(!c.0.is_empty() && !c.2.is_empty()); }
    }

    #[test]
    fn run_checks_not_truncated() { assert!(!run_unx49081_checks().truncated()); }

    #[test]
    fn run_checks_len_eight() { assert_eq!(run_unx49081_checks().len(), 8); }

    #[test]
    fn run_checks_passed_eight() { assert_eq!(run_unx49081_checks().passed_count(), 8); }

    #[test]
    fn run_checks_failed_zero() { assert_eq!(run_unx49081_checks().failed_count(), 0); }

    #[test]
    fn run_checks_domain_carries_f49081() { assert!(run_unx49081_checks().domain.contains("F49081")); }

    #[test]
    fn parse_u32_rejects_empty() { assert_eq!(parse_u32(""), None); }

    #[test]
    fn parse_u32_rejects_overflow() { assert_eq!(parse_u32("4294967296"), None); }

    #[test]
    fn parse_u32_accepts_max() { assert_eq!(parse_u32("4294967295"), Some(u32::MAX)); }

    #[test]
    fn parse_u32_accepts_zero() { assert_eq!(parse_u32("0"), Some(0)); }

    #[test]
    fn parse_u32_rejects_non_digit() {
        for s in ["12a", "-1", " 1", "1.0"] { assert_eq!(parse_u32(s), None); }
    }

    #[test]
    fn parse_hex16_requires_sixteen_chars() {
        assert_eq!(parse_hex16("0"), None);
        assert_eq!(parse_hex16(""), None);
    }

    #[test]
    fn parse_hex16_is_case_insensitive() {
        assert_eq!(parse_hex16("00000000000000ff"), Some(255));
        assert_eq!(parse_hex16("00000000000000FF"), Some(255));
    }

    #[test]
    fn parse_hex16_rejects_non_hex() { assert_eq!(parse_hex16("000000000000000g"), None); }

    #[test]
    fn parse_hex16_accepts_max() { assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX)); }

    #[test]
    fn row_fields_and_render_are_canonical() {
        let row = render_row(&DefRow { dev: 0x8A, stage: 3, enq: 8, del: 3, drop: 5 });
        assert!(row.starts_with("X3D81|dev=138|stage=3|enq=8|del=3|drop=5|digest=") && row.len() == 64);
        assert!(row.ends_with(&format!("{:016x}", row_digest(0x8A, 3, 8, 3, 5))));
        let f = row_fields(&row, DEF_TAG, &DEF_KEYS).expect("fields");
        assert_eq!((f.len(), f[0], f[4], f[5].len()), (DEF_KEYS.len() + 1, "138", "5", 16));
    }

    #[test]
    fn row_fields_rejects_malformed() {
        let bad = ["X3D81|dev=1", "X3D99|dev=1|stage=1|enq=1|del=0|drop=1|digest=0000000000000000", "X3D81|dev=|stage=1|enq=1|del=0|drop=1|digest=0000000000000000", "X3D81|dev=1|stage=1|enq=1|del=0|drop=1|digest=00"];
        for row in bad { assert!(row_fields(row, DEF_TAG, &DEF_KEYS).is_none()); }
    }

    #[test]
    fn audit_accepts_good_and_rejects_tamper() {
        let rows = export_rows();
        assert_eq!(rows.len(), HOT_ROUNDS as usize);
        for r in &rows { assert!(audit_row(r)); }
        assert!(!audit_row(&format!("{}x", rows[0])));
        assert!(!audit_row(&rows[0].replace(DEF_TAG, "X3D99")));
        assert!(!audit_row(&rows[0].replace("digest=", "digest=0")));
        assert!(!audit_row(&render_row(&DefRow { dev: 0x81, stage: 1, enq: 9, del: 1, drop: 7 })));
        assert!(!audit_row(&format!("{}|dev=129|stage=4|enq=1|del=0|drop=1|digest={:016x}", DEF_TAG, row_digest(0x81, 4, 1, 0, 1))));
    }

    #[test]
    fn balanced_rejects_overdelivery() { assert!(!DefRow { dev: 0x81, stage: 1, enq: 1, del: 2, drop: 0 }.balanced()); }

    #[test]
    fn export_rows_unique_and_stable() {
        let rows = export_rows();
        for (i, r) in rows.iter().enumerate() { for m in rows.iter().skip(i + 1) { assert_ne!(r, m); } }
        assert!(rows_stable());
    }

    #[test]
    fn row_digest_is_deterministic_and_sensitive() {
        assert_eq!(row_digest(0x81, 1, 4, 1, 3), row_digest(0x81, 1, 4, 1, 3));
        assert_ne!(row_digest(0x81, 1, 4, 1, 3), row_digest(0x82, 1, 4, 1, 3));
        assert_ne!(row_digest(0x81, 1, 4, 1, 3), row_digest(0x81, 2, 4, 1, 3));
    }

    #[test]
    fn governor_stages_and_bounds() {
        let mut g = Governor::new();
        assert_eq!((g.tokens(0), g.tokens(1), g.tokens(2)), (CAP_L3, CAP_L2, CAP_L1));
        let mut n = 0u32;
        while g.admit(1) == Verdict::Admit { n += 1; }
        assert_eq!((n, g.tokens(0), g.tokens(1), g.tokens(2)), (CAP_L1, CAP_L3 - CAP_L1, CAP_L2 - CAP_L1, 0));
        let before = g.total_tokens();
        g.tick();
        assert_eq!((g.total_tokens(), g.ticks), (before + REFILL_L3 + REFILL_L2 + REFILL_L1, 1));
        assert!(g.partition_ok() && g.residue_ok());
    }

    #[test]
    fn governor_rejects_invalid_and_saturates() {
        let mut g = Governor::new();
        for cost in [0u32, MAX_COST + 1, u32::MAX] { assert_eq!(g.admit(cost), Verdict::Invalid); }
        assert_eq!((g.invalid, g.tokens(0)), (3, CAP_L3));
        assert_eq!(g.admit(MAX_COST), Verdict::Admit);
        assert!(g.fill(2, u32::MAX) && g.tokens(2) == CAP_L1);
        assert!(g.fill(1, 3) && g.tokens(1) == 3 && !g.fill(STAGES, 1));
        assert!(g.residue_ok() && g.partition_ok());
    }

    #[test]
    fn flood_counters_partition_and_saturate() {
        let mut g = Governor::new();
        let mut c = FloodCounters::new();
        for i in 0u32..40 {
            let cost = i % (MAX_COST + 1);
            let v = g.admit(cost);
            c.observe(v, cost);
        }
        assert_eq!(c.seen, 40);
        assert!(c.partition_ok() && c.invalid > 0 && c.throttled > 0 && c.admitted > 0 && c.drop > 0);
        assert!(c.max_cost <= MAX_COST);
        c.seen = u32::MAX - 1;
        c.observe(Verdict::Admit, 1);
        c.observe(Verdict::Admit, 1);
        assert_eq!((c.seen, sat_add(u32::MAX, 9)), (u32::MAX, u32::MAX));
    }

    #[test]
    fn evidence_ring_overwrites_oldest() {
        let mut r = EvidenceRing::new();
        for i in 0u32..20 { r.push(Evidence { dev: HOT_DEV_BASE, seq: i, cost: 1, verdict: Verdict::Admit }); }
        assert_eq!((r.len(), r.overwritten), (FORENSIC_SLOTS, 4));
        assert!(r.coherent() && !r.is_empty() && r.get(FORENSIC_SLOTS).is_none());
        assert_eq!((r.get(0).map(|e| e.seq), r.newest().map(|e| e.seq)), (Some(4), Some(19)));
    }

    #[test]
    fn evidence_validity_bounds() {
        assert!(Evidence { dev: HOT_DEV_BASE, seq: 0, cost: 1, verdict: Verdict::Admit }.valid());
        assert!(!Evidence { dev: 0, seq: 0, cost: 1, verdict: Verdict::Admit }.valid());
        assert!(!Evidence { dev: HOT_DEV_BASE, seq: 0, cost: 0, verdict: Verdict::Admit }.valid());
        assert!(!Evidence { dev: HOT_DEV_BASE, seq: 0, cost: 9, verdict: Verdict::Admit }.valid());
        assert!(!Evidence { dev: HOT_DEV_BASE, seq: u32::MAX, cost: 1, verdict: Verdict::Admit }.valid());
    }

    #[test]
    fn hot_single_rounds_are_clean() {
        for i in 1..=HOT_ROUNDS {
            let r = hot_remove_round(i);
            assert!(r.clean(), "round {} not clean", i);
            assert_eq!(r.drop, r.enq - r.del);
        }
    }

    #[test]
    fn hot_x10_zero_residue() {
        let reps = hot_remove_x10();
        assert_eq!(reps.len(), HOT_ROUNDS as usize);
        for (i, r) in reps.iter().enumerate() {
            assert_eq!(r.round, i as u32 + 1);
            assert_eq!((r.state, r.residue, r.leaked, r.refused, r.transitions), (HotState::Removed, 0, 0, 0, 4));
            assert_eq!(r.drop, r.enq - r.del);
        }
        assert!(hot_remove_clean());
    }

    #[test]
    fn hot_drop_accounting_reconciles() {
        let reps = hot_remove_x10();
        let (enq, del, drop) = reps.iter().fold((0u32, 0u32, 0u32), |a, r| (a.0 + r.enq, a.1 + r.del, a.2 + r.drop));
        assert_eq!(enq, del + drop);
        assert!(drop > 0 && drop == hot_drop_total());
        assert!(reps.iter().all(|r| r.accounted()));
    }

    #[test]
    fn hot_queue_overflow_is_rejected() {
        let mut q = Inflight::new();
        for i in 0..QUEUE_SLOTS + 3 { q.enqueue(Frame { dev: HOT_DEV_BASE, seq: i as u32, cost: 1 }); }
        assert_eq!((q.len(), q.refs() as usize, q.leaked()), (QUEUE_SLOTS, QUEUE_SLOTS, 0));
        assert_eq!(q.drop_all(), QUEUE_SLOTS as u32);
        assert!(q.is_empty() && q.refs() == 0 && q.leaked() == 0 && q.pop().is_none());
    }

    #[test]
    fn hot_transitions_allow_only_error_paths() {
        let good = [(HotState::Present, HotState::Draining), (HotState::Present, HotState::Removed), (HotState::Draining, HotState::Removed), (HotState::Removed, HotState::Resetting), (HotState::Resetting, HotState::Removed), (HotState::Resetting, HotState::Live), (HotState::Live, HotState::Present)];
        for (a, b) in good { assert!(transition_allowed(a, b)); }
        let bad = [(HotState::Present, HotState::Live), (HotState::Removed, HotState::Draining), (HotState::Live, HotState::Removed), (HotState::Removed, HotState::Present)];
        for (a, b) in bad { assert!(!transition_allowed(a, b)); }
    }

    #[test]
    fn hot_advance_counts_refusals() {
        let mut st = HotState::Present;
        let (mut ok, mut bad) = (0u32, 0u32);
        advance(&mut st, HotState::Draining, &mut ok, &mut bad);
        advance(&mut st, HotState::Present, &mut ok, &mut bad);
        assert_eq!((st, ok, bad, HotState::Removed.tag()), (HotState::Draining, 1, 1, "removed"));
    }

    #[test]
    fn reorder_rounds_account_for_every_frame() {
        for i in 1..=REORDER_ROUNDS {
            let r = reorder_round(i);
            assert!(r.clean(), "round {} not clean", i);
            assert_eq!(r.delivered + r.flushed, r.planned);
        }
        let reps = reorder_x10();
        assert_eq!(reps.len(), REORDER_ROUNDS as usize);
        for (i, r) in reps.iter().enumerate() {
            assert_eq!((r.round, r.resets, r.refused), (i as u32 + 1, 1, 0));
            assert!(r.accounted());
        }
        assert!(reorder_clean() && reps.iter().any(|r| r.flushed > 0));
    }

    #[test]
    fn reorder_holds_duplicates_and_releases() {
        let mut w = ReorderWindow::new();
        assert_eq!(w.observe(2), Delivery::Held);
        assert_eq!(w.observe(2), Delivery::Duplicate);
        assert_eq!((w.held_len(), w.duplicates), (1, 1));
        assert_eq!(w.observe(0), Delivery::InOrder);
        assert_eq!(w.observe(1), Delivery::InOrder);
        assert_eq!((w.delivered, w.held_len()), (3, 0));
        assert_eq!(w.observe(0), Delivery::Duplicate);
        assert_eq!(w.duplicates, 2);
    }

    #[test]
    fn reorder_gap_is_flushed_by_reset() {
        let mut w = ReorderWindow::new();
        for i in 1..=5u32 { w.observe(i); }
        assert_eq!(w.held_len(), 5);
        assert_eq!(w.reset_flow(), 5);
        assert_eq!((w.held_len(), w.resets, w.expect()), (0, 1, 0));
        assert_eq!(w.reset_flow(), 0);
        assert_eq!(w.resets, 2);
    }

    #[test]
    fn rows_stable_and_mix_deterministic() {
        assert!(rows_stable());
        assert_eq!(mix(0x81, 1), mix(0x81, 1));
        assert_ne!(mix(0x81, 1), mix(0x81, 2));
        assert_ne!(mix(0x81, 1), mix(0x82, 1));
    }
}
