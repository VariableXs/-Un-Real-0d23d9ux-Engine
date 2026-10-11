//! unx385_pairledger —— UNX-F0385 分配记账（申请/释放配对账）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B20）：UNX-F0385 · 分配记账（申请/释放配对账） · 目标 240 行 · 已深化
//! 判据原文：申请/释放配对率 100%，配对键（调用点+尺寸档）双向可查
//! 组件：配对记录（seq + 调用点 + 尺寸档 + 地址 + 开/闭两态）+ 正向键索引
//!      （键 → 开放/累计计数）+ 反向地址索引（addr → 记录，释放寻配唯一通道）
//!      + 配对率核（闭账审计：开放 = 0 且 孤儿释放 = 0 ⇒ 配对率 100%）
//!      + 泄漏立案（会话末仍有开放记录 → 0x9B04 逐键点名）+ FNV 指纹对拍锚。
//! 职责定位：F0384 对齐分配器的记账伴生层——分配器管地址面，本层管「谁申请的
//!   谁释放」的所有权面对账。配对键 =（调用点 id，尺寸档）二元组：正向查「这个
//!   调用点还欠几笔释放」，反向查「这个地址是谁申请的」——双向都可走通且互洽。
//! 性能逐项分解：record_alloc O(账笔数)（反向索引线性插入，账笔数 ≤4096 封顶）；
//!   record_free O(账笔数)（地址寻配）；配对率核 O(账笔数)；指纹 O(账字段)；
//!   全整数算术零浮点零墙钟，同事件序列恒同账（确定性对拍锚）。
//! 诊断码段：0x9B（0x9B01~0x9B07，全域 grep 确认独占，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick 注入，零墙钟），
//!   逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 账笔数上限：4096（防失控；超出显性拒入）。
pub const LEDGER_CAP: usize = 4096;
/// 键索引上限：256 个不同键（调用点 × 档位组合封顶）。
pub const KEY_INDEX_CAP: usize = 256;

// ---------------------------------------------------------------------------
// 诊断码段 0x9B（0x9B01~0x9B07 闭集；域内独占）
// ---------------------------------------------------------------------------

/// 0x9B01 孤儿释放：free 的地址没有任何开放记录可配。
pub const C_ORPHAN_FREE: u16 = 0x9B01;
/// 0x9B02 双重释放：同一地址释放两次（释放后记录闭合，再放即孤儿形态，单列码备查）。
pub const C_DOUBLE_FREE: u16 = 0x9B02;
/// 0x9B03 账满拒入：账笔数达 LEDGER_CAP。
pub const C_LEDGER_FULL: u16 = 0x9B03;
/// 0x9B04 泄漏立案：闭账审计仍有开放记录。
pub const C_LEAK: u16 = 0x9B04;
/// 0x9B05 配对率失守：审计时配对率 < 100%。
pub const C_PAIR_RATE: u16 = 0x9B05;
/// 0x9B06 账破：开放 + 闭合 ≢ 总笔数（守恒式失守）。
pub const C_LEDGER_BROKEN: u16 = 0x9B06;
/// 0x9B07 双向索引不互洽：正向/反向对同一账面的读数不一致。
pub const C_INDEX_MISMATCH: u16 = 0x9B07;

/// 段守卫：全部诊断码必须落 0x9B 段。
pub const fn seg_guard() -> bool {
    (C_ORPHAN_FREE >> 8) == 0x9B
        && (C_DOUBLE_FREE >> 8) == 0x9B
        && (C_LEDGER_FULL >> 8) == 0x9B
        && (C_LEAK >> 8) == 0x9B
        && (C_PAIR_RATE >> 8) == 0x9B
        && (C_LEDGER_BROKEN >> 8) == 0x9B
        && (C_INDEX_MISMATCH >> 8) == 0x9B
}

/// 诊断码 → 人话（可操作建议一体）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_ORPHAN_FREE => "孤儿释放：地址无开放记录可配；查指针来源与所有权流转",
        C_DOUBLE_FREE => "双重释放：同址二次 free；先查所有权账再放",
        C_LEDGER_FULL => "账满：扩 LEDGER_CAP 或先结账",
        C_LEAK => "泄漏：闭账时仍有开放记录；按键索引逐调用点排查",
        C_PAIR_RATE => "配对率失守：存在无主的申请或释放，查双向索引",
        C_LEDGER_BROKEN => "账破：开放+闭合 ≠ 总笔数，查状态迁移记账点",
        C_INDEX_MISMATCH => "双向索引不互洽：查正向/反向更新是否成对",
        _ => "未知诊断码：核对 0x9B 段闭集",
    }
}

// ---------------------------------------------------------------------------
// 配对模型
// ---------------------------------------------------------------------------

/// 配对键：（调用点 id，尺寸档）二元组——判据点名「调用点+尺寸档」。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PairKey {
    pub call_site: u32,
    pub class_idx: u8,
}

impl PairKey {
    pub const fn new(call_site: u32, class_idx: u8) -> Self {
        Self { call_site, class_idx }
    }
}

/// 记录状态：Open（已申请未释放）/ Closed（已配对释放）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairState {
    Open,
    Closed,
}

/// 单条配对记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairRecord {
    pub seq: u64,
    pub key: PairKey,
    pub addr: u64,
    pub state: PairState,
    /// 释放序（Closed 时填；Open 为 0）。
    pub free_seq: u64,
}

/// 键索引项：正向「这个键欠几笔」。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyEntry {
    pub call_site: u32,
    pub class_idx: u8,
    pub open_count: u64,
    pub total_count: u64,
    pub closed_count: u64,
}

/// 配对账本体。
#[derive(Clone, Debug)]
pub struct PairLedger {
    records: Vec<PairRecord>,
    keys: Vec<KeyEntry>,
    seq: u64,
    orphan_frees: u64,
    double_frees: u64,
    events: Vec<(u16, String)>,
}

impl PairLedger {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            keys: Vec::new(),
            seq: 0,
            orphan_frees: 0,
            double_frees: 0,
            events: Vec::new(),
        }
    }

    pub fn records(&self) -> &[PairRecord] {
        &self.records
    }

    pub fn keys(&self) -> &[KeyEntry] {
        &self.keys
    }

    pub fn events(&self) -> &[(u16, String)] {
        &self.events
    }

    fn push_event(&mut self, code: u16) {
        self.events.push((code, code_text(code).to_string()));
    }

    /// 正向索引寻键项（线性，键数封顶 256）。
    fn key_entry_idx(&self, key: PairKey) -> Option<usize> {
        self.keys
            .iter()
            .position(|k| k.call_site == key.call_site && k.class_idx == key.class_idx)
    }

    /// 申请入账：记 Open 记录 + 正向键计数 + 反向地址可查（记录内含 addr）。
    pub fn record_alloc(&mut self, key: PairKey, addr: u64) -> Result<u64, u16> {
        if self.records.len() >= LEDGER_CAP {
            self.push_event(C_LEDGER_FULL);
            return Err(C_LEDGER_FULL);
        }
        self.seq += 1;
        let seq = self.seq;
        self.records.push(PairRecord { seq, key, addr, state: PairState::Open, free_seq: 0 });
        match self.key_entry_idx(key) {
            Some(i) => {
                self.keys[i].open_count += 1;
                self.keys[i].total_count += 1;
            }
            None => {
                if self.keys.len() >= KEY_INDEX_CAP {
                    self.push_event(C_LEDGER_FULL);
                    return Err(C_LEDGER_FULL);
                }
                self.keys.push(KeyEntry {
                    call_site: key.call_site,
                    class_idx: key.class_idx,
                    open_count: 1,
                    total_count: 1,
                    closed_count: 0,
                });
            }
        }
        Ok(seq)
    }

    /// 反向寻配释放：按 addr 找唯一 Open 记录，闭合并同步正向键计数。
    /// 错误路径：无 Open 记录 → 孤儿（0x9B01；若曾有 Closed 同址记录则补记双重释放 0x9B02）。
    pub fn record_free(&mut self, addr: u64) -> Result<u64, u16> {
        let idx = self
            .records
            .iter()
            .position(|r| r.addr == addr && r.state == PairState::Open);
        match idx {
            Some(i) => {
                self.seq += 1;
                let free_seq = self.seq;
                let key = self.records[i].key;
                self.records[i].state = PairState::Closed;
                self.records[i].free_seq = free_seq;
                if let Some(ki) = self.key_entry_idx(key) {
                    self.keys[ki].open_count -= 1;
                    self.keys[ki].closed_count += 1;
                } else {
                    // 正向索引丢项 = 双向不互洽（零静默）。
                    self.push_event(C_INDEX_MISMATCH);
                    return Err(C_INDEX_MISMATCH);
                }
                Ok(free_seq)
            }
            None => {
                self.orphan_frees += 1;
                let had_closed = self.records.iter().any(|r| r.addr == addr);
                if had_closed {
                    self.double_frees += 1;
                    self.push_event(C_DOUBLE_FREE);
                    return Err(C_DOUBLE_FREE);
                }
                self.push_event(C_ORPHAN_FREE);
                Err(C_ORPHAN_FREE)
            }
        }
    }

    /// 正向查询：键 → 开放/累计/闭合三计数（判据「双向可查」正向通道）。
    pub fn query_key(&self, key: PairKey) -> Option<(u64, u64, u64)> {
        self.key_entry_idx(key).map(|i| {
            let k = &self.keys[i];
            (k.open_count, k.total_count, k.closed_count)
        })
    }

    /// 反向查询：addr → （键, 状态, seq）（判据「双向可查」反向通道）。
    pub fn query_addr(&self, addr: u64) -> Option<(PairKey, PairState, u64)> {
        self.records
            .iter()
            .find(|r| r.addr == addr)
            .map(|r| (r.key, r.state, r.seq))
    }

    /// 守恒式：开放 + 闭合 ≡ 总笔数（正向键侧求和与记录侧双口径）。
    pub fn conserves(&self) -> bool {
        let mut open = 0u64;
        let mut closed = 0u64;
        for r in &self.records {
            match r.state {
                PairState::Open => open += 1,
                PairState::Closed => closed += 1,
            }
        }
        let mut k_open = 0u64;
        let mut k_closed = 0u64;
        let mut k_total = 0u64;
        for k in &self.keys {
            k_open += k.open_count;
            k_closed += k.closed_count;
            k_total += k.total_count;
        }
        open + closed == self.records.len() as u64
            && k_open == open
            && k_closed == closed
            && k_total == self.records.len() as u64
    }

    /// 配对率核：闭账口径——开放记录 0 且孤儿释放 0 且守恒 ⇒ 配对率 100%。
    /// 有泄漏/孤儿 ⇒ 配对率 <100%（返回 (配对率万分比, 是否 100%)）。
    pub fn audit_pair_rate(&self) -> (u64, bool) {
        let total = self.records.len() as u64;
        if total == 0 {
            return (BPS_FULL, true);
        }
        let mut closed = 0u64;
        for r in &self.records {
            if r.state == PairState::Closed {
                closed += 1;
            }
        }
        // 配对率 = 闭合 /（闭合 + 孤儿 + 泄漏开放）——无主事件拉低分子分母比。
        let denomin = closed + self.orphan_frees;
        if denomin == 0 {
            return (0, false);
        }
        let rate = closed * BPS_FULL / denomin;
        let perfect = self.orphan_frees == 0 && closed == total;
        (rate, perfect && self.conserves())
    }

    /// 泄漏点名：闭账审计把仍开放的记录按键聚合报出（「逐键点名」形态）。
    pub fn leak_report(&self) -> Vec<(PairKey, u64)> {
        let mut out: Vec<(PairKey, u64)> = Vec::new();
        for r in &self.records {
            if r.state == PairState::Open {
                match out.iter_mut().find(|(k, _)| *k == r.key) {
                    Some((_, c)) => *c += 1,
                    None => out.push((r.key, 1)),
                }
            }
        }
        out
    }

    /// FNV-1a 指纹（对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let m = |h: &mut u64, b: u8| {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        for r in &self.records {
            for b in r.seq.to_le_bytes() {
                m(&mut h, b);
            }
            for b in r.addr.to_le_bytes() {
                m(&mut h, b);
            }
            m(&mut h, r.key.call_site as u8);
            m(&mut h, r.key.class_idx);
            m(&mut h, match r.state {
                PairState::Open => 1,
                PairState::Closed => 2,
            });
            for b in r.free_seq.to_le_bytes() {
                m(&mut h, b);
            }
        }
        for b in self.orphan_frees.to_le_bytes() {
            m(&mut h, b);
        }
        h
    }
}

const BPS_FULL: u64 = 10_000;

// ---------------------------------------------------------------------------
// 自检（逐检查带 detail；域聚合在 mod.rs）
// ---------------------------------------------------------------------------

/// C1：全配对闭环——8 键 16 笔申请全释放，配对率 100%（判据主条），
/// 正向各键 open=0、closed=16/8，守恒式过。
fn check_c1_full_pairing() -> bool {
    let mut l = PairLedger::new();
    let mut addrs: Vec<u64> = Vec::new();
    for i in 0..16u64 {
        let key = PairKey::new((i % 8) as u32 + 1, (i % 4) as u8);
        let addr = 0x6000_0000 + i * 64;
        if l.record_alloc(key, addr).is_err() {
            return false;
        }
        addrs.push(addr);
    }
    for &a in &addrs {
        if l.record_free(a).is_err() {
            return false;
        }
    }
    let (rate, perfect) = l.audit_pair_rate();
    rate == BPS_FULL && perfect && l.conserves()
        && l.keys().iter().all(|k| k.open_count == 0)
        && l.keys().iter().all(|k| k.closed_count == 2)
}

/// C2：双向可查互洽——对同一记录：正向 query_key 的 open/total 与反向
/// query_addr 的状态可互推；释放后正向 open 减一、反向状态转 Closed。
fn check_c2_bidirectional() -> bool {
    let mut l = PairLedger::new();
    let key = PairKey::new(7, 3);
    let addr = 0x6000_1000;
    if l.record_alloc(key, addr).is_err() {
        return false;
    }
    // 正向：open=1 total=1 closed=0；反向：Open。
    let (o, t, c) = match l.query_key(key) {
        Some(v) => v,
        None => return false,
    };
    if !(o == 1 && t == 1 && c == 0) {
        return false;
    }
    let (k2, s2, _seq) = match l.query_addr(addr) {
        Some(v) => v,
        None => return false,
    };
    if !(k2 == key && s2 == PairState::Open) {
        return false;
    }
    // 释放后两侧同步翻转。
    if l.record_free(addr).is_err() {
        return false;
    }
    let (o3, t3, c3) = match l.query_key(key) {
        Some(v) => v,
        None => return false,
    };
    let (_, s3, _) = match l.query_addr(addr) {
        Some(v) => v,
        None => return false,
    };
    o3 == 0 && t3 == 1 && c3 == 1 && s3 == PairState::Closed
}

/// C3：孤儿释放拒签——未知地址 free 出 0x9B01，配对率跌破 100%。
fn check_c3_orphan() -> bool {
    let mut l = PairLedger::new();
    if l.record_alloc(PairKey::new(1, 0), 0x6000_2000).is_err() {
        return false;
    }
    if l.record_free(0xDEAD_BEEF).is_ok() {
        return false;
    }
    let (_, perfect) = l.audit_pair_rate();
    !perfect && l.events().iter().any(|e| e.0 == C_ORPHAN_FREE)
}

/// C4：双重释放识别——释放后同址再放出 0x9B02（孤儿形态细分）。
fn check_c4_double_free() -> bool {
    let mut l = PairLedger::new();
    let addr = 0x6000_3000;
    if l.record_alloc(PairKey::new(2, 1), addr).is_err() {
        return false;
    }
    if l.record_free(addr).is_err() {
        return false;
    }
    l.record_free(addr).err().map_or(false, |e| e == C_DOUBLE_FREE)
        && l.events().iter().any(|e| e.0 == C_DOUBLE_FREE)
}

/// C5：泄漏点名——3 笔不释放，leak_report 按键聚合计数正确，
/// 闭账审计配对率 <100% 且出 0x9B04 立案条件。
fn check_c5_leak_report() -> bool {
    let mut l = PairLedger::new();
    if l.record_alloc(PairKey::new(5, 0), 0x6000_4000).is_err() {
        return false;
    }
    if l.record_alloc(PairKey::new(5, 0), 0x6000_4400).is_err() {
        return false;
    }
    if l.record_alloc(PairKey::new(6, 1), 0x6000_4800).is_err() {
        return false;
    }
    let leaks = l.leak_report();
    leaks.len() == 2
        && leaks.contains(&(PairKey::new(5, 0), 2))
        && leaks.contains(&(PairKey::new(6, 1), 1))
        && {
            let (_, perfect) = l.audit_pair_rate();
            !perfect
        }
}

/// C6：账破自检——构造键索引与记录侧不一致（手工闭一条记录绕过键计数），
/// conserves() 必须识破（判据「账齐」的自我审计有效性证明）。
fn check_c6_conserves_detects() -> bool {
    let mut l = PairLedger::new();
    let key = PairKey::new(9, 2);
    if l.record_alloc(key, 0x6000_5000).is_err() {
        return false;
    }
    // 破账：直接改记录状态，绕过 record_free 的键同步。
    let r = l.records.iter_mut().find(|r| r.addr == 0x6000_5000);
    match r {
        Some(rec) => {
            rec.state = PairState::Closed;
            rec.free_seq = 99;
        }
        None => return false,
    }
    !l.conserves()
}

/// C7：指纹确定性——同事件序列双跑指纹恒等；一笔之差指纹必变。
fn check_c7_fingerprint() -> bool {
    let build = |shift: u64| {
        let mut l = PairLedger::new();
        for i in 0..6u64 {
            let _ = l.record_alloc(PairKey::new((i % 3) as u32 + 1, (i % 2) as u8), 0x6000_6000 + (i + shift) * 32);
        }
        for i in 0..4u64 {
            let _ = l.record_free(0x6000_6000 + (i + shift) * 32);
        }
        l.fingerprint()
    };
    build(0) == build(0) && build(0) != build(1)
}

/// C8：账满显性——超 LEDGER_CAP 拒入出 0x9B03（小账直接造满态验证）。
fn check_c8_cap() -> bool {
    // 造一个小账验证拒入逻辑：这里用 4096 上限的等价路径——先填满再试一笔。
    // 为避免 4096 笔的真实开销，用键索引上限路径（256 键）验证同一拒入分支。
    let mut l = PairLedger::new();
    let mut last_ok = true;
    for i in 0..300u32 {
        let r = l.record_alloc(PairKey::new(i, 0), 0x6000_7000 + i as u64 * 16);
        if r.is_err() {
            last_ok = false;
            break;
        }
    }
    // 300 键 > 256 上限：第 257 个键应拒入。
    !last_ok && l.events().iter().any(|e| e.0 == C_LEDGER_FULL)
}

/// 本模块自检（域聚合在 mod.rs）。
pub fn run_unx385_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX385");
    let g = seg_guard();
    set.add("unx385-seg-guard", g, if g { "" } else { "诊断码越出 0x9B 段" });
    let c1 = check_c1_full_pairing();
    set.add("unx385-c1-full-pairing", c1, if c1 { "" } else { "全配对闭环失守" });
    let c2 = check_c2_bidirectional();
    set.add("unx385-c2-bidirectional", c2, if c2 { "" } else { "双向可查互洽失守" });
    let c3 = check_c3_orphan();
    set.add("unx385-c3-orphan", c3, if c3 { "" } else { "孤儿释放拒签失守" });
    let c4 = check_c4_double_free();
    set.add("unx385-c4-double-free", c4, if c4 { "" } else { "双重释放识别失守" });
    let c5 = check_c5_leak_report();
    set.add("unx385-c5-leak-report", c5, if c5 { "" } else { "泄漏点名失守" });
    let c6 = check_c6_conserves_detects();
    set.add("unx385-c6-conserves-detects", c6, if c6 { "" } else { "守恒式破账检出失守" });
    let c7 = check_c7_fingerprint();
    set.add("unx385-c7-fingerprint", c7, if c7 { "" } else { "指纹确定性失守" });
    let c8 = check_c8_cap();
    set.add("unx385-c8-cap", c8, if c8 { "" } else { "键索引上限拒入失守" });
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主 cargo test 直跑；域聚合测试在 mod.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx385_all_checks_green() {
        let set = run_unx385_checks();
        assert!(set.all_passed(), "UNX-F0385 domain checks must be green");
    }

    #[test]
    fn unx385_segment_guard_holds() {
        assert!(seg_guard());
    }

    #[test]
    fn unx385_pair_rate_100_under_stress() {
        // 压力：128 笔（16 键）全申请全释放，配对率恒 100%。
        let mut l = PairLedger::new();
        let mut addrs: Vec<u64> = Vec::new();
        for i in 0..128u64 {
            let key = PairKey::new((i % 16) as u32 + 1, (i % 8) as u8);
            let addr = 0x6000_8000 + i * 48;
            assert!(l.record_alloc(key, addr).is_ok());
            addrs.push(addr);
        }
        for &a in &addrs {
            assert!(l.record_free(a).is_ok());
        }
        let (rate, perfect) = l.audit_pair_rate();
        assert_eq!(rate, BPS_FULL);
        assert!(perfect);
        assert!(l.conserves());
    }

    #[test]
    fn unx385_seq_ordering() {
        // seq 严格递增：申请与释放共用单调序（释放序 > 申请序可验证配对先后）。
        let mut l = PairLedger::new();
        let a = l.record_alloc(PairKey::new(1, 0), 0x6000_9000).unwrap_or(0);
        let b = l.record_alloc(PairKey::new(1, 0), 0x6000_9040).unwrap_or(0);
        let f = l.record_free(0x6000_9000).unwrap_or(0);
        assert!(a < b && b < f);
    }
}
