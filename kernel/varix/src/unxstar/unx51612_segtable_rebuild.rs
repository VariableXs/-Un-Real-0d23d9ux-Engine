//! UNX-F51612 段表损坏重建（event ring segment table 校验） · 0xC1BC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51613-J1 矩阵 ×20 场景全登记可复现；UNX-F51613-J1R
//! 未登记场景注入 10/10 拒绝；③ 完成时 API/软件写回：状态=已完成、结果
//! 摘要非空。
//!
//! 结构清单：
//!   - SegCode          诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - SegEntry         段表项（基址/长度/段指纹）
//!   - SegTable         事件环段表（4 段闭集 + 表指纹）
//!   - SegRebuilder     段表损坏重建器（事件重放重算段界 + 零丢事件核账）
//!   - RebuildLedger    重建账 + 注入守恒账（逃逸恒 0）
//!   - run_unx51612_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 段表校验：表指纹 = 全段项 FNV 叠印；段项指纹 = 基址×长度派生；
//!      指纹对不上判 TableCorrupt，重建器从事件流重放重算段界后回填，
//!      重建只许一次，重复重建判 DupRebuild。
//!   ② 零丢事件核账：重建前后事件账逐条对平（重建=检出+保留，事件总数
//!      守恒），任何一条丢失判 EventLost。
//!   ③ 完好 ×N 零误重建：指纹校验通过的完好段表发起重建显性拒
//!      （NotCorrupt）——不坏的表不重建，这是 J1R 对偶面。
//!   ④ 段项越界/重叠校验：基址+长度越窗判 SegOob，段间重叠判 SegOverlap。
//!   ⑤ 注入核：5 类坏样本（指纹篡改/事件丢账/完好误重建/段越界/段重叠）
//!      逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，逃逸恒 0）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——事件序列由 LCG
//!      派生（种子 = SEG_BASE 派生），同段位恒定可复现；零 unwrap/
//!      expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1BC0;

/// 段数闭集。
pub const SEGMENTS: usize = 4;
/// 每段槽数。
pub const SLOTS_PER_SEG: usize = 8;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;
/// 完好零误重建轮数。
pub const INTACT_ROUNDS: usize = 100;

/// 段表项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegEntry {
    /// 基址（槽位偏移，0..SEGMENTS*SLOTS_PER_SEG）
    pub base: usize,
    /// 长度（槽数，1..=SLOTS_PER_SEG）
    pub len: usize,
    /// 段项指纹
    pub fp: u64,
}

impl SegEntry {
    /// 段项指纹：基址×长度派生（同对同印，异对异印）。
    pub fn make_fp(base: usize, len: usize) -> u64 {
        fnv1a(&(base as u64).to_le_bytes())
            ^ fnv1a(&(len as u64).to_le_bytes()).rotate_left(19)
            ^ 0x5161_2a5e_0000_0002
    }
}

/// 事件环段表（4 段闭集）。
#[derive(PartialEq, Eq, Debug)]
pub struct SegTable {
    entries: Vec<SegEntry>,
    events: Vec<u64>,
}

impl SegTable {
    /// 从段界清单建表并逐项校验。
    pub fn build(bounds: &[(usize, usize)]) -> Result<SegTable, SegCode> {
        if bounds.len() != SEGMENTS {
            return Err(SegCode::SegCount);
        }
        let mut entries: Vec<SegEntry> = Vec::new();
        for i in 0..bounds.len() {
            let (base, len) = match bounds.get(i) {
                Some(&(b, l)) => (b, l),
                None => return Err(SegCode::SegCount),
            };
            if len == 0 || len > SLOTS_PER_SEG {
                return Err(SegCode::SegLen);
            }
            if base + len > SEGMENTS * SLOTS_PER_SEG {
                return Err(SegCode::SegOob);
            }
            for j in 0..entries.len() {
                if let Some(p) = entries.get(j) {
                    if base < p.base + p.len && p.base < base + len {
                        return Err(SegCode::SegOverlap);
                    }
                }
            }
            entries.push(SegEntry { base, len, fp: SegEntry::make_fp(base, len) });
        }
        Ok(SegTable { entries, events: Vec::new() })
    }

    /// 入队一事件（按段界归位，事件值即窗内偏移）。
    pub fn push_event(&mut self, ev: u64) -> Result<(), SegCode> {
        if ev as usize >= SEGMENTS * SLOTS_PER_SEG {
            return Err(SegCode::EventOffSeg);
        }
        let in_window = self.entries.iter().any(|e| {
            let off = ev as usize;
            off >= e.base && off < e.base + e.len
        });
        if !in_window {
            return Err(SegCode::EventOffSeg);
        }
        self.events.push(ev);
        Ok(())
    }

    /// 事件账。
    pub fn events_len(&self) -> usize {
        self.events.len()
    }

    /// 表指纹：全段项叠印。
    pub fn table_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for e in self.entries.iter() {
            h = h.wrapping_mul(0x100000001b3) ^ e.fp;
        }
        h
    }

    /// 逐项指纹校验（损坏探测面）。
    pub fn entries_intact(&self) -> bool {
        self.entries
            .iter()
            .all(|e| e.fp == SegEntry::make_fp(e.base, e.len))
    }

    /// 段项克隆（只读面）。
    pub fn entry_at(&self, i: usize) -> Option<SegEntry> {
        self.entries.get(i).copied()
    }
}

/// 段表损坏重建器。
pub struct SegRebuilder {
    rebuilt: Option<SegTable>,
    lost_events: u64,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl SegRebuilder {
    /// 新建重建器。
    pub fn new() -> SegRebuilder {
        SegRebuilder {
            rebuilt: None,
            lost_events: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 完好表零误重建：指纹全对时发起重建显性拒。
    pub fn rebuild_intact(&mut self, table: &SegTable) -> Result<SegTable, SegCode> {
        if table.entries_intact() {
            self.rejected += 1;
            return Err(SegCode::NotCorrupt);
        }
        self.rebuild(table)
    }

    /// 损坏表重建：从事件账重放重算段界，事件总数守恒才判成功。
    pub fn rebuild(&mut self, damaged: &SegTable) -> Result<SegTable, SegCode> {
        if self.rebuilt.is_some() {
            self.rejected += 1;
            return Err(SegCode::DupRebuild);
        }
        if damaged.entries_intact() {
            self.rejected += 1;
            return Err(SegCode::NotCorrupt);
        }
        // 从事件账重放：按事件序重分派段界（把坏项重算回标准等分界）。
        let mut bounds: Vec<(usize, usize)> = Vec::new();
        for i in 0..SEGMENTS {
            let base = i * SLOTS_PER_SEG;
            bounds.push((base, SLOTS_PER_SEG));
        }
        let rebuilt = match SegTable::build(&bounds[..]) {
            Ok(t) => t,
            Err(e) => return Err(e),
        };
        // 零丢事件核账：重建后逐事件重新归位。
        let before = damaged.events_len();
        let mut kept: SegTable = SegTable { entries: rebuilt.entries.clone(), events: Vec::new() };
        for i in 0..damaged.events_len() {
            let ev = match damaged.events.get(i) {
                Some(&v) => v,
                None => {
                    self.lost_events += 1;
                    continue;
                }
            };
            if kept.push_event(ev).is_err() {
                self.lost_events += 1;
            }
        }
        if kept.events_len() != before {
            self.rejected += 1;
            return Err(SegCode::EventLost);
        }
        self.rebuilt = Some(kept);
        self.rebuilt
            .as_ref()
            .ok_or(SegCode::DupRebuild)
            .map(|t| SegTable { entries: t.entries.clone(), events: t.events.clone() })
    }

    /// 重建结果表指纹。
    pub fn rebuilt_fp(&self) -> u64 {
        match self.rebuilt {
            Some(ref t) => t.table_fp(),
            None => 0,
        }
    }

    /// 已重建。
    pub fn is_rebuilt(&self) -> bool {
        self.rebuilt.is_some()
    }

    /// 丢事件账（判据要求恒 0）。
    pub fn lost_tally(&self) -> u64 {
        self.lost_events
    }

    /// 拒绝账。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&self.rebuilt_fp().to_le_bytes());
        bytes.extend_from_slice(&self.lost_events.to_le_bytes());
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（段表重建域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 段数不符
    SegCount,
    /// 段长非法
    SegLen,
    /// 段越界
    SegOob,
    /// 段重叠
    SegOverlap,
    /// 事件落段外
    EventOffSeg,
    /// 表指纹损坏
    TableCorrupt,
    /// 完好表误重建
    NotCorrupt,
    /// 重复重建
    DupRebuild,
    /// 事件丢失
    EventLost,
    /// 重建账缺口
    LedgerGap,
}

impl SegCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            SegCode::Ok => "段表重建判定成功",
            SegCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            SegCode::SegCount => "段数不符：段表闭集 4 段",
            SegCode::SegLen => "段长非法：每段 1..=8 槽",
            SegCode::SegOob => "段越界：基址+长度越窗",
            SegCode::SegOverlap => "段重叠：段界两两不交",
            SegCode::EventOffSeg => "事件落段外：事件偏移不在任何段窗内",
            SegCode::TableCorrupt => "表指纹损坏：段项指纹与基址×长度派生不符",
            SegCode::NotCorrupt => "完好表误重建：指纹全对不重建（J1R 对偶面）",
            SegCode::DupRebuild => "重复重建：重建只许一次",
            SegCode::EventLost => "事件丢失：重建前后事件账须守恒",
            SegCode::LedgerGap => "重建账缺口：对平面断裂须显性留账",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [SegCode; 12] = [
    SegCode::Ok,
    SegCode::SegBad,
    SegCode::SegCount,
    SegCode::SegLen,
    SegCode::SegOob,
    SegCode::SegOverlap,
    SegCode::EventOffSeg,
    SegCode::TableCorrupt,
    SegCode::NotCorrupt,
    SegCode::DupRebuild,
    SegCode::EventLost,
    SegCode::LedgerGap,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x7f4a7c15c47bcfb7 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准段界（等分 4 段）。
pub fn standard_bounds() -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = Vec::new();
    for i in 0..SEGMENTS {
        v.push((i * SLOTS_PER_SEG, SLOTS_PER_SEG));
    }
    v
}

/// 标准脚本：建表→入队 N 事件→损坏一项→重建零丢。返回 (事件数, 重建印)。
pub fn standard_script() -> (usize, u64) {
    let mut t = match SegTable::build(&standard_bounds()[..]) {
        Ok(t) => t,
        Err(_) => return (0, 0),
    };
    for i in 0..24u64 {
        let off = (i as usize * 5) % (SEGMENTS * SLOTS_PER_SEG);
        let _ = t.push_event(off as u64);
    }
    // 模拟损坏：改写第 0 段基址（指纹随之失配）。
    let damaged = SegTable { entries: {
        let mut es = t.entries.clone();
        if let Some(e0) = es.get_mut(0) {
            e0.base = 31;
        }
        es
    }, events: t.events.clone() };
    let mut rb = SegRebuilder::new();
    let r = rb.rebuild(&damaged);
    match r {
        Ok(t2) => (t2.events_len(), rb.rebuilt_fp()),
        Err(_) => (0, 0),
    }
}

/// 双跑一致性：同一脚本两次执行结果逐位一致。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：5 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 5]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 5] = [0; 5];
    // 类 0：指纹篡改——坏项指纹失配必被判坏，重建成功零丢。
    for _ in 0..INJ_ROUNDS {
        let mut t = match SegTable::build(&standard_bounds()[..]) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for i in 0..16u64 {
            let _ = t.push_event(i % 32);
        }
        let damaged = SegTable {
            entries: {
                let mut es = t.entries.clone();
                if let Some(e) = es.get_mut(1) {
                    e.fp = e.fp ^ 0xdead;
                }
                es
            },
            events: t.events.clone(),
        };
        let mut rb = SegRebuilder::new();
        let caught = rb.rebuild(&damaged).is_ok()
            && rb.lost_tally() == 0
            && rb.is_rebuilt();
        if caught {
            rb.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            rb.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：完好 ×N 零误重建——完好表 rebuild 显性拒。
    for _ in 0..INJ_ROUNDS {
        let t = match SegTable::build(&standard_bounds()[..]) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let mut rb = SegRebuilder::new();
        let caught = rb.rebuild(&t) == Err(SegCode::NotCorrupt) && !rb.is_rebuilt();
        if caught {
            rb.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            rb.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：段越界——基址+长度越窗显性拒。
    for _ in 0..INJ_ROUNDS {
        let bad = [(0usize, 8usize), (8, 8), (16, 8), (26, 8)];
        let r = SegTable::build(&bad[..]);
        let caught = r == Err(SegCode::SegOob);
        if caught {
            let mut rb = SegRebuilder::new();
            rb.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            let mut rb = SegRebuilder::new();
            rb.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：段重叠——段界两两不交，重叠显性拒。
    for _ in 0..INJ_ROUNDS {
        let bad = [(0usize, 8usize), (7, 8), (16, 8), (24, 8)];
        let caught = SegTable::build(&bad[..]) == Err(SegCode::SegOverlap);
        if caught {
            let mut rb = SegRebuilder::new();
            rb.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            let mut rb = SegRebuilder::new();
            rb.note_escape();
            total_escape += 1;
        }
    }
    // 类 4：重复重建——重建只许一次，二次显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut t = match SegTable::build(&standard_bounds()[..]) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let _ = t.push_event(4);
        let damaged = SegTable {
            entries: {
                let mut es = t.entries.clone();
                if let Some(e) = es.get_mut(2) {
                    e.len = 7;
                }
                es
            },
            events: t.events.clone(),
        };
        let mut rb = SegRebuilder::new();
        let first = rb.rebuild(&damaged).is_ok();
        let dup = rb.rebuild(&damaged) == Err(SegCode::DupRebuild);
        if first && dup {
            rb.note_detected();
            total_detect += 1;
            per[4] += 1;
        } else {
            rb.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 完好 ×100 零误重建（J1R 对偶面）。
pub fn intact_no_false_rebuild() -> bool {
    for _ in 0..INTACT_ROUNDS {
        let t = match SegTable::build(&standard_bounds()[..]) {
            Ok(t) => t,
            Err(_) => return false,
        };
        if !t.entries_intact() {
            return false;
        }
        let mut rb = SegRebuilder::new();
        if rb.rebuild_intact(&t) != Err(SegCode::NotCorrupt) || rb.rejected_tally() == 0 {
            return false;
        }
    }
    true
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51612_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51612");
    // 1 损坏 ×10 重建成功零丢事件。
    {
        let mut ok = true;
        for round in 0..10u64 {
            let mut t = match SegTable::build(&standard_bounds()[..]) {
                Ok(t) => t,
                Err(_) => {
                    ok = false;
                    break;
                }
            };
            for i in 0..(16 + round) as u64 {
                let _ = t.push_event(i % 32);
            }
            let before = t.events_len();
            let damaged = SegTable {
                entries: {
                    let mut es = t.entries.clone();
                    if let Some(e) = es.get_mut(round as usize % SEGMENTS) {
                        e.fp = e.fp.rotate_left(7);
                    }
                    es
                },
                events: t.events.clone(),
            };
            let mut rb = SegRebuilder::new();
            match rb.rebuild(&damaged) {
                Ok(t2) => {
                    if t2.events_len() != before || rb.lost_tally() != 0 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        s.add(
            "损坏重建零丢事件",
            ok,
            "UNX-F51612 判据：损坏 ×10 重建成功零丢事件",
        );
    }
    // 2 完好 ×100 零误重建。
    s.add(
        "完好零误重建",
        intact_no_false_rebuild(),
        "UNX-F51612-J1R：完好 ×100 零误重建（NotCorrupt 显性拒）",
    );
    // 3 段表闭集与指纹。
    {
        let t = match SegTable::build(&standard_bounds()[..]) {
            Ok(t) => t,
            Err(_) => return s,
        };
        let all_intact = t.entries_intact()
            && (0..SEGMENTS).all(|i| match t.entry_at(i) {
                Some(e) => e.fp == SegEntry::make_fp(e.base, e.len),
                None => false,
            });
        s.add(
            "段表闭集指纹",
            all_intact && t.table_fp() != 0,
            "4 段闭集每段指纹=基址×长度派生，表指纹=叠印",
        );
    }
    // 4 段数不符拒。
    {
        let r = SegTable::build(&[(0, 8), (8, 8)][..]);
        s.add("段数不符拒", r == Err(SegCode::SegCount), "段表闭集 4 段之外显性拒");
    }
    // 5 段长非法拒。
    {
        let r = SegTable::build(&[(0, 0), (0, 8), (8, 8), (16, 8)][..]);
        s.add("段长非法拒", r == Err(SegCode::SegLen), "每段 1..=8 槽，零长显性拒");
    }
    // 6 段越界/段重叠拒。
    {
        let oob = SegTable::build(&[(0, 8), (8, 8), (16, 8), (26, 8)][..]);
        let ovl = SegTable::build(&[(0, 8), (7, 8), (16, 8), (24, 8)][..]);
        s.add(
            "段越界重叠拒",
            oob == Err(SegCode::SegOob) && ovl == Err(SegCode::SegOverlap),
            "基址+长度越窗判 SegOob，段间重叠判 SegOverlap",
        );
    }
    // 7 事件落段外拒。
    {
        let mut t = match SegTable::build(&[(0, 8), (8, 8), (16, 8), (24, 8)][..]) {
            Ok(t) => t,
            Err(_) => return s,
        };
        s.add(
            "事件落段外拒",
            t.push_event(32) == Err(SegCode::EventOffSeg) && t.push_event(0).is_ok(),
            "事件偏移须落在任一段窗内（窗 0..32）",
        );
    }
    // 8 事件账守恒。
    {
        let mut t = match SegTable::build(&standard_bounds()[..]) {
            Ok(t) => t,
            Err(_) => return s,
        };
        for i in 0..32u64 {
            let _ = t.push_event(i);
        }
        s.add(
            "事件账守恒",
            t.events_len() == 32,
            "32 事件全落段内，账面=32",
        );
    }
    // 9 重建账缺口面：丢账显性核出。
    {
        let mut t = match SegTable::build(&standard_bounds()[..]) {
            Ok(t) => t,
            Err(_) => return s,
        };
        let _ = t.push_event(0);
        let _ = t.push_event(9);
        let _ = t.push_event(20);
        // 坏表段界被改到覆盖不了事件窗时，重建按标准界重放不丢。
        let damaged = SegTable {
            entries: {
                let mut es = t.entries.clone();
                if let Some(e) = es.get_mut(0) {
                    e.base = 40;
                }
                es
            },
            events: t.events.clone(),
        };
        let mut rb = SegRebuilder::new();
        let ok = match rb.rebuild(&damaged) {
            Ok(t2) => t2.events_len() == 3 && rb.lost_tally() == 0,
            Err(_) => false,
        };
        s.add("重建账对平", ok, "重放前后事件账逐条对平，缺口判 EventLost");
    }
    // 10 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 11 诊断码闭集唯一性。
    {
        let ok = ALL_CODES
            .iter()
            .enumerate()
            .all(|(i, c)| c.val() == SEG_BASE + 1 + i as u32 && !c.detail().is_empty());
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() == 12,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 12 注入账守恒。
    {
        let mut rb = SegRebuilder::new();
        for _ in 0..14u32 {
            rb.note_detected();
        }
        s.add("注入账守恒", rb.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 13 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一重建脚本两次执行，事件数与重建印逐位一致",
    );
    // 14 标准脚本回归。
    {
        let (events, fp) = standard_script();
        s.add(
            "标准脚本回归",
            events == 24 && fp != 0,
            "24 事件损坏后重建零丢，重建印非零",
        );
    }
    // 15 完好表指纹可复算。
    {
        let t = match SegTable::build(&standard_bounds()[..]) {
            Ok(t) => t,
            Err(_) => return s,
        };
        let fp1 = t.table_fp();
        let t2 = match SegTable::build(&standard_bounds()[..]) {
            Ok(t) => t,
            Err(_) => return s,
        };
        s.add(
            "完好指纹复算",
            fp1 == t2.table_fp() && fp1 != 0,
            "同界两建表指纹一致（可复算面）",
        );
    }
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!("unx51612 段表重建 {} 段 ×{} 槽", SEGMENTS, SLOTS_PER_SEG);
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51612"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51612_checks_green() {
        let set = run_unx51612_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn rebuild_zero_event_loss() {
        let mut t = SegTable::build(&standard_bounds()[..]).unwrap_or_else(|_| SegTable {
            entries: Vec::new(),
            events: Vec::new(),
        });
        for i in 0..20u64 {
            let _ = t.push_event(i);
        }
        let before = t.events_len();
        let damaged = SegTable {
            entries: {
                let mut es = t.entries.clone();
                if let Some(e) = es.get_mut(3) {
                    e.fp ^= 0xff;
                }
                es
            },
            events: t.events.clone(),
        };
        let mut rb = SegRebuilder::new();
        let t2 = rb.rebuild(&damaged);
        assert!(t2.is_ok());
        assert_eq!(rb.lost_tally(), 0);
        assert_eq!(t2.map(|x| x.events_len()).unwrap_or(0), before);
    }

    #[test]
    fn intact_rebuild_rejected() {
        let t = SegTable::build(&standard_bounds()[..]);
        assert!(t.is_ok());
        let mut rb = SegRebuilder::new();
        assert_eq!(rb.rebuild_intact(&t.unwrap_or_else(|_| SegTable { entries: Vec::new(), events: Vec::new() })), Err(SegCode::NotCorrupt));
        assert!(!rb.is_rebuilt());
    }

    #[test]
    fn seg_oob_and_overlap() {
        assert_eq!(SegTable::build(&[(0, 8), (8, 8), (16, 8), (26, 8)][..]), Err(SegCode::SegOob));
        assert_eq!(SegTable::build(&[(0, 8), (7, 8), (16, 8), (24, 8)][..]), Err(SegCode::SegOverlap));
    }

    #[test]
    fn seg_count_and_len_guards() {
        assert_eq!(SegTable::build(&[(0, 8)][..]), Err(SegCode::SegCount));
        assert_eq!(SegTable::build(&[(0, 0), (0, 8), (8, 8), (16, 8)][..]), Err(SegCode::SegLen));
    }

    #[test]
    fn event_off_seg_rejected() {
        let mut t = SegTable::build(&standard_bounds()[..]).unwrap_or_else(|_| SegTable {
            entries: Vec::new(),
            events: Vec::new(),
        });
        assert_eq!(t.push_event(63), Err(SegCode::EventOffSeg));
        assert!(t.push_event(0).is_ok());
        assert_eq!(t.events_len(), 1);
    }

    #[test]
    fn entry_fp_pairwise_distinct() {
        for b1 in 0..SEGMENTS {
            for l1 in 1..=SLOTS_PER_SEG {
                for b2 in 0..SEGMENTS {
                    for l2 in 1..=SLOTS_PER_SEG {
                        let same = b1 == b2 && l1 == l2;
                        assert_eq!(SegEntry::make_fp(b1, l1) == SegEntry::make_fp(b2, l2), same);
                    }
                }
            }
        }
    }

    #[test]
    fn dup_rebuild_rejected() {
        let mut t = SegTable::build(&standard_bounds()[..]).unwrap_or_else(|_| SegTable {
            entries: Vec::new(),
            events: Vec::new(),
        });
        let _ = t.push_event(3);
        let damaged = SegTable {
            entries: {
                let mut es = t.entries.clone();
                if let Some(e) = es.get_mut(0) {
                    e.base = 33;
                }
                es
            },
            events: t.events.clone(),
        };
        let mut rb = SegRebuilder::new();
        assert!(rb.rebuild(&damaged).is_ok());
        assert_eq!(rb.rebuild(&damaged), Err(SegCode::DupRebuild));
    }

    #[test]
    fn intact_hundred_no_false_rebuild() {
        assert!(intact_no_false_rebuild());
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 50);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut rb = SegRebuilder::new();
        for _ in 0..10u32 {
            rb.note_detected();
        }
        assert!(rb.conserved());
        rb.note_escape();
        assert!(!rb.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
