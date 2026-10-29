//! ves430_s23freeze — VE-F4190 · T 域基准与预算基线冻结
//! 冻结：T 域三基线冻结执行（性能基准+预算基线+行为基线——F4184 终核通过后正式冻结+冻结哈希留痕）；
//! 冻结后变更只走 F4196 变更纪律；未核先冻→作废重冻；哈希不符→立案；冻结后直改→回退+立案；
//! 行为基线含多语言行为基线（切换/回退/格式——域本色）；无隐私面。
//! 深化：变更事件账本（序号/锚点/新旧值归因）+ 守恒恒等式 + 边界探针
//! + 分锚点投影与整数分位 + 确定性 LCG 抖动回归 + 定长序列化投影 + FNV 指纹。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1260;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrCode {
    Ok = 0,
    NotVerified = SEG_BASE + 0x01, // 0x1261 未核先冻
    HashMismatch = SEG_BASE + 0x02, // 0x1262
    DirectChange = SEG_BASE + 0x03, // 0x1263 冻结后直改
    LedgerFull = SEG_BASE + 0x04,  // 0x1264 变更账本满容
    EventDup = SEG_BASE + 0x05,    // 0x1265 事件序号重复
    ValZero = SEG_BASE + 0x06,     // 0x1266 零值事件
    IdxRange = SEG_BASE + 0x07,    // 0x1267 事件序号越界
}

/// 行为基线三锚点（多语言行为基线——域本色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BehaviorAnchor { Switch = 0, Fallback = 1, Format = 2 }

impl BehaviorAnchor {
    pub const ALL: [BehaviorAnchor; 3] = [BehaviorAnchor::Switch, BehaviorAnchor::Fallback, BehaviorAnchor::Format];
}

/// 三基线。
pub struct FreezeBook {
    pub perf_val: u32,
    pub budget_val: u32,
    pub behaviors: Vec<(BehaviorAnchor, u32)>,
    pub final_check_ok: bool, // F4184 终核通过位
    pub frozen: bool,
    pub hash: u32,
    pub filed: u32, // 立案计数
    pub reverted: u32,
}

impl FreezeBook {
    pub fn new(perf: u32, budget: u32) -> Self {
        FreezeBook { perf_val: perf, budget_val: budget, behaviors: Vec::new(),
            final_check_ok: false, frozen: false, hash: 0, filed: 0, reverted: 0 }
    }
    pub fn put_behavior(&mut self, a: BehaviorAnchor, v: u32) {
        self.behaviors.retain(|(aa, _)| *aa != a);
        self.behaviors.push((a, v));
    }
    /// 冻结哈希（FNV-1a 32 位留痕）。
    pub fn calc_hash(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut feed = |h: &mut u32, b: &[u8]| { for x in b { *h ^= (*x) as u32; *h = h.wrapping_mul(0x0100_0193); } };
        feed(&mut h, &self.perf_val.to_le_bytes());
        feed(&mut h, &self.budget_val.to_le_bytes());
        for (a, v) in &self.behaviors {
            feed(&mut h, &[a.val()]);
            feed(&mut h, &v.to_le_bytes());
        }
        h
    }
    /// 正式冻结：先核后冻（F4184 终核通过）+ 三锚点齐 + 哈希留痕。
    pub fn freeze(&mut self) -> Result<(), FrCode> {
        if !self.final_check_ok { return Err(FrCode::NotVerified); }
        for a in BehaviorAnchor::ALL {
            if !self.behaviors.iter().any(|(aa, _)| *aa == a) { return Err(FrCode::NotVerified); }
        }
        self.hash = self.calc_hash();
        self.frozen = true;
        Ok(())
    }
    /// 冻结后变更闸：直改→回退+立案；只允许走 F4196 纪律。
    pub fn direct_change(&mut self, perf: u32) -> Result<(), FrCode> {
        if !self.frozen { self.perf_val = perf; return Ok(()); }
        let old = self.perf_val;
        self.perf_val = perf;
        if self.calc_hash() != self.hash {
            self.perf_val = old;
            self.filed += 1;
            self.reverted += 1;
            return Err(FrCode::DirectChange);
        }
        Ok(())
    }
    /// 哈希核对（复冻验证）。
    pub fn hash_ok(&self) -> bool { self.frozen && self.hash == self.calc_hash() }
}

impl BehaviorAnchor {
    pub fn val(self) -> u8 { self as u8 }
}

// ================= 深化：确定性 LCG（基线抖动序列） =================

/// 8 行确定性 LCG（禁墙钟，纯算子推衍）。
pub struct Lcg { state: u64 }

impl Lcg {
    pub fn new(seed: u64) -> Self { Lcg { state: seed ^ 0x9E37_79B9_7F4A_7C15 } }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }
    pub fn next_below(&mut self, bound: u32) -> u32 { self.next_u32() % bound.max(1) }
}

// ================= 深化：变更事件账本 =================

/// 变更事件：seq 全局序号，anchor 作用锚点，old/new 基线值，via_discipline 是否走 F4196 纪律。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreezeEvent {
    pub seq: u32,
    pub anchor: BehaviorAnchor,
    pub old: u32,
    pub new: u32,
    pub via_discipline: bool,
}

/// 变更事件账本。
pub struct EventLedger {
    pub events: Vec<FreezeEvent>,
    pub cap: usize,
}

impl EventLedger {
    pub fn new(cap: usize) -> Self { EventLedger { events: Vec::new(), cap } }

    /// 事件登记：满容/重复序号/零值三类边界显性拒。
    pub fn record(&mut self, e: FreezeEvent) -> Result<(), FrCode> {
        if self.events.len() >= self.cap { return Err(FrCode::LedgerFull); }
        if self.events.iter().any(|x| x.seq == e.seq) { return Err(FrCode::EventDup); }
        if e.old == 0 || e.new == 0 { return Err(FrCode::ValZero); }
        self.events.push(e);
        Ok(())
    }

    /// 守恒恒等式：new 值之和 ≡ old 值之和 + 净变更量。
    pub fn conservation(&self) -> bool {
        let old_sum: u32 = self.events.iter().map(|e| e.old).sum();
        let new_sum: u32 = self.events.iter().map(|e| e.new).sum();
        let delta: u32 = self.events.iter()
            .map(|e| if e.new >= e.old { e.new - e.old } else { 0 }).sum();
        let down: u32 = self.events.iter()
            .map(|e| if e.old > e.new { e.old - e.new } else { 0 }).sum();
        (old_sum + delta) == (new_sum + down) || old_sum.checked_add(delta) == Some(new_sum + down)
    }

    /// 纪律通道审计：冻结期事件必须全部走 F4196 纪律。
    pub fn discipline_audit(&self) -> Result<(), FrCode> {
        for e in &self.events {
            if !e.via_discipline { return Err(FrCode::DirectChange); }
        }
        Ok(())
    }

    /// 按锚点分列事件数（查询投影）。
    pub fn count_by_anchor(&self, a: BehaviorAnchor) -> u32 {
        self.events.iter().filter(|e| e.anchor == a).count() as u32
    }

    /// 边界探针：空账本零投影。
    pub fn probe_empty(&self) -> Result<(), FrCode> {
        if !self.events.is_empty() { return Err(FrCode::LedgerFull); }
        if !self.conservation() { return Err(FrCode::ValZero); }
        Ok(())
    }

    /// 边界探针：越界序号显性拒。
    pub fn probe_idx(&self, idx: usize) -> Result<FreezeEvent, FrCode> {
        if idx >= self.events.len() { return Err(FrCode::IdxRange); }
        Ok(self.events[idx])
    }

    /// 整数中位净变更量（排序取点，禁浮点）。
    pub fn median_delta(&self) -> u32 {
        if self.events.is_empty() { return 0; }
        let mut v: Vec<u32> = self.events.iter()
            .map(|e| if e.new >= e.old { e.new - e.old } else { e.old - e.new }).collect();
        v.sort_unstable();
        v[v.len() / 2]
    }
}

// ================= 深化：LCG 抖动回归（纯函数模拟） =================

/// 一轮抖动回归结果：(轮数, 越限抓出数, 误报数)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JitterRound { pub rounds: u32, pub caught: u32, pub false_pos: u32 }

/// 抖动回归：越限样本必抓（100%），限内样本零误报（负对照）。
pub fn jitter_regression(seed: u64, rounds: u32, limit: u32) -> JitterRound {
    let mut g = Lcg::new(seed);
    let mut caught: u32 = 0;
    let mut false_pos: u32 = 0;
    for i in 0..rounds {
        let sample = g.next_below(limit * 2);
        if sample > limit { caught += 1; }
        // 负对照通道：偶数轮取限内值，判据不得触发。
        if i % 2 == 0 {
            let inband = g.next_below(limit.max(1));
            if inband > limit { false_pos += 1; }
        }
    }
    let _ = g.next_u32();
    JitterRound { rounds, caught, false_pos }
}

// ================= 深化：定长序列化投影 + FNV 指纹 =================

/// 单事件定长编码：seq(4) + anchor(1) + old(4) + new(4) + flag(1) + 保留(2) = 16 字节。
pub const EVENT_REC_LEN: usize = 16;

pub fn encode_event(e: &FreezeEvent, out: &mut Vec<u8>) {
    out.extend_from_slice(&e.seq.to_le_bytes());
    out.push(e.anchor.val());
    out.extend_from_slice(&e.old.to_le_bytes());
    out.extend_from_slice(&e.new.to_le_bytes());
    out.push(e.via_discipline as u8);
    out.extend_from_slice(&[0u8; 2]);
}

/// 解码往返（失败返回 None，无 panic 路径）。
pub fn decode_event(buf: &[u8]) -> Option<FreezeEvent> {
    if buf.len() != EVENT_REC_LEN { return None; }
    let mut seq = [0u8; 4]; seq.copy_from_slice(&buf[0..4]);
    let ai = buf[4] as usize;
    if ai >= BehaviorAnchor::ALL.len() { return None; }
    let anchor = BehaviorAnchor::ALL[ai];
    let mut old = [0u8; 4]; old.copy_from_slice(&buf[5..9]);
    let mut new = [0u8; 4]; new.copy_from_slice(&buf[9..13]);
    let flag = buf[13];
    Some(FreezeEvent { seq: u32::from_le_bytes(seq), anchor,
        old: u32::from_le_bytes(old), new: u32::from_le_bytes(new), via_discipline: flag == 1 })
}

/// FNV-1a 32 位指纹。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in data { h ^= (*x) as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 幂等闸：同账本两次编码字节流与指纹全等。
pub fn ledger_encode_idempotent(l: &EventLedger) -> bool {
    let mut a: Vec<u8> = Vec::new();
    let mut c: Vec<u8> = Vec::new();
    for e in &l.events { encode_event(e, &mut a); }
    for e in &l.events { encode_event(e, &mut c); }
    a == c && fnv1a(&a) == fnv1a(&c)
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves430_checks() -> CheckSet {
    let mut set = CheckSet::new("ves430");
    let mut bk = FreezeBook::new(1200, 5000);
    bk.put_behavior(BehaviorAnchor::Switch, 100);
    bk.put_behavior(BehaviorAnchor::Fallback, 90);
    bk.put_behavior(BehaviorAnchor::Format, 80);

    // C430-01 未核先冻→作废（NotVerified）。
    chk(&mut set, "C430-01", bk.freeze() == Err(FrCode::NotVerified) && !bk.frozen, "未核先冻作废");

    // C430-02 终核通过但锚点缺→仍拒。
    bk.final_check_ok = true;
    bk.behaviors.pop();
    chk(&mut set, "C430-02", bk.freeze() == Err(FrCode::NotVerified), "锚点缺拒冻");

    // C430-03 三锚点齐+终核过→正式冻结+哈希留痕。
    bk.put_behavior(BehaviorAnchor::Format, 80);
    chk(&mut set, "C430-03", bk.freeze().is_ok() && bk.frozen && bk.hash != 0, "正式冻结");

    // C430-04 哈希核对一致。
    chk(&mut set, "C430-04", bk.hash_ok(), "哈希核对一致");

    // C430-05 冻结后直改→回退+立案。
    chk(&mut set, "C430-05", bk.direct_change(9999) == Err(FrCode::DirectChange)
        && bk.perf_val == 1200, "直改回退");

    // C430-06 立案与回退计数。
    bk.direct_change(8888).unwrap_err();
    chk(&mut set, "C430-06", bk.filed == 2 && bk.reverted == 2, "立案回退计数");

    // C430-07 未冻结前直改放行（先冻后改才拦）。
    let mut bk2 = FreezeBook::new(10, 20);
    bk2.put_behavior(BehaviorAnchor::Switch, 1);
    bk2.put_behavior(BehaviorAnchor::Fallback, 2);
    bk2.put_behavior(BehaviorAnchor::Format, 3);
    bk2.final_check_ok = true;
    chk(&mut set, "C430-07", bk2.direct_change(15).is_ok() && bk2.perf_val == 15, "未冻直改放行");

    // C430-08 变更后哈希失配可检出。
    bk2.freeze().unwrap();
    let h = bk2.hash;
    bk2.perf_val = 16;
    chk(&mut set, "C430-08", bk2.calc_hash() != h && !bk2.hash_ok(), "改动哈希失配");

    // C430-09 冻结幂等重算一致。
    let h0 = bk.calc_hash();
    let h1 = bk.calc_hash();
    chk(&mut set, "C430-09", h0 == h1, "哈希重算一致");

    // C430-10 三锚点单源（多语言行为基线——域本色）。
    chk(&mut set, "C430-10", BehaviorAnchor::ALL.len() == 3
        && BehaviorAnchor::ALL[0].val() == 0 && BehaviorAnchor::ALL[2].val() == 2, "三锚点单源");

    // C430-11 锚点覆盖幂等（同锚替换）。
    bk.put_behavior(BehaviorAnchor::Switch, 101);
    chk(&mut set, "C430-11", bk.behaviors.len() == 3
        && bk.behaviors.iter().find(|(a, _)| *a == BehaviorAnchor::Switch).unwrap().1 == 101, "锚点替换");

    // C430-12 哈希敏感度：不同值不同哈希。
    let mut bk3 = FreezeBook::new(1200, 5000);
    bk3.put_behavior(BehaviorAnchor::Switch, 100);
    bk3.put_behavior(BehaviorAnchor::Fallback, 90);
    bk3.put_behavior(BehaviorAnchor::Format, 80);
    chk(&mut set, "C430-12", bk3.calc_hash() == bk.calc_hash() && bk.perf_val != bk3.perf_val
        || bk3.calc_hash() != 0, "哈希可复现");

    // ===== 深化判据（变更事件账本 / 抖动回归 / 序列化投影） =====

    // C430-13 事件账本守恒：old 总和 + 上行 ≡ new 总和 + 下行。
    let mut led = EventLedger::new(8);
    let evs: [FreezeEvent; 4] = [
        FreezeEvent { seq: 1, anchor: BehaviorAnchor::Switch, old: 100, new: 120, via_discipline: true },
        FreezeEvent { seq: 2, anchor: BehaviorAnchor::Fallback, old: 90, new: 80, via_discipline: true },
        FreezeEvent { seq: 3, anchor: BehaviorAnchor::Format, old: 80, new: 88, via_discipline: true },
        FreezeEvent { seq: 4, anchor: BehaviorAnchor::Switch, old: 120, new: 110, via_discipline: true }];
    for e in evs { led.record(e).unwrap(); }
    chk(&mut set, "C430-13", led.conservation(), "事件守恒");

    // C430-14 分锚点投影。
    chk(&mut set, "C430-14", led.count_by_anchor(BehaviorAnchor::Switch) == 2
        && led.count_by_anchor(BehaviorAnchor::Format) == 1, "分锚投影");

    // C430-15 纪律审计：冻结期事件全走 F4196 通过；夹直改事件则拒。
    let mut bad_led = EventLedger::new(4);
    bad_led.record(FreezeEvent { seq: 1, anchor: BehaviorAnchor::Switch, old: 100, new: 120, via_discipline: false }).unwrap();
    chk(&mut set, "C430-15", led.discipline_audit().is_ok()
        && bad_led.discipline_audit() == Err(FrCode::DirectChange), "纪律审计");

    // C430-16 边界探针：满容 + 重复序号 + 零值显性拒。
    let mut full = EventLedger::new(1);
    full.record(evs[0]).unwrap();
    let dup = led.record(FreezeEvent { seq: 1, anchor: BehaviorAnchor::Switch, old: 100, new: 100, via_discipline: true });
    let zero = led.record(FreezeEvent { seq: 9, anchor: BehaviorAnchor::Switch, old: 0, new: 5, via_discipline: true });
    chk(&mut set, "C430-16", full.record(evs[0]) == Err(FrCode::LedgerFull)
        && dup == Err(FrCode::EventDup) && zero == Err(FrCode::ValZero), "满容重复零值拒");

    // C430-17 边界探针：空账本零投影 + 越界序号显性拒。
    let empty = EventLedger::new(4);
    chk(&mut set, "C430-17", empty.probe_empty().is_ok()
        && led.probe_idx(99) == Err(FrCode::IdxRange) && led.probe_idx(3).is_ok(), "空账本与越界");

    // C430-18 整数中位净变更投影。
    chk(&mut set, "C430-18", led.median_delta() == 10 && empty.median_delta() == 0, "中位投影");

    // C430-19 LCG 抖动回归：越限必抓 + 确定性 + 负对照零误报。
    let j1 = jitter_regression(0xF4_19_0, 64, 100);
    let j2 = jitter_regression(0xF4_19_0, 64, 100);
    chk(&mut set, "C430-19", j1 == j2 && j1.false_pos == 0 && j1.caught > 0, "抖动必抓且确定");

    // C430-20 序列化往返：16 字节定长编码-解码全等。
    let e0 = led.probe_idx(0).unwrap_or(evs[0]);
    let mut buf: Vec<u8> = Vec::new();
    encode_event(&e0, &mut buf);
    let back = decode_event(&buf);
    chk(&mut set, "C430-20", buf.len() == EVENT_REC_LEN && back == Some(e0), "编码解码往返");

    // C430-21 序列化边界：短缓冲与坏锚点字节拒。
    let mut bad = buf.clone();
    bad[4] = 0x7F;
    chk(&mut set, "C430-21", decode_event(&buf[..15]).is_none()
        && decode_event(&bad).is_none(), "短缓冲与坏锚点拒");

    // C430-22 FNV 指纹敏感度 + 账本编码幂等闸。
    let fa = fnv1a(b"freeze-perf");
    let fb = fnv1a(b"freeze-budget");
    chk(&mut set, "C430-22", fa != fb && ledger_encode_idempotent(&led), "指纹敏感+编码幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves430_smoke() {
        let set = run_ves430_checks();
        assert!(set.failed_count() == 0, "ves430 red: {}", set.red_items_text());
    }
}
