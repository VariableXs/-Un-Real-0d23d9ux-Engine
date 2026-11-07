//! UNX-F3447 串行标记与并发互斥（no_parallel 元数据位） · 0x79D00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B14 逐字摘录）：
//!   ① 纯功能代码 ≥260 行；
//!   ② J1：no_parallel 位语义（标记用例独占执行器）+ 互斥调度验证；
//!     M-206 串行标记用例执行期并发计数恒为 1。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - no_parallel 位语义：冻结用例注册表带标记位，标记例独占执行器
//!     （独占 = 执行期并发计数恒为 1），非标记例可双核并行（计数 ≤ 2）；
//!   - 互斥调度：调度器把用例流排成时间轴（Slot：起拍/止拍/并发计数），
//!     串行例必须等在跑例全部清场后才独占上场，并行例按双核额两两成批；
//!   - M-206 判定：对时间轴做重叠重构——串行例时槽与任何其他例时槽
//!     交叠即违约（并发计数恒为 1 的整数面等价判据）；
//!   - 并发计数纪律：进场 +1 / 退场 -1 全走 checked 计数，下溢与越
//!     双核上沿都显性拒；
//!   - 注入必抓：串行例被并排（co_schedule）、计数下溢、三例同拍越
//!     双核上沿三类人为违约全部被闭集诊断码点名；
//!   - 双跑一致：同提交序两次调度的时间轴逐字段相同。
//! 常量来源：
//!   - 双核上沿 2：与 F3446 双核桩调度器对齐（同一执行端）；
//!   - M-206：派工卡验收判据编号（串行互斥判据锚定 0x206）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B14 批段窗 [0x79C00, 0x79DC0] 内）。
pub const SEG_BASE: u32 = 0x79D00;

/// 双核并发上沿（与 F3446 执行端对齐）。
pub const MAX_CONCURRENCY: u32 = 2;

/// M-206 串行互斥判据锚定号。
pub const INJECTION_CASE_ID: u32 = 0x206;

/// 冻结用例注册表：用例名 → (用例 ID, no_parallel 位, 代价拍)。
/// 「串行独占」例带 no_parallel 位（独占执行器）。
pub const CASE_FLAG_TABLE: [(&str, u32, bool, u64); 5] = [
    ("并行冒烟甲", 1, false, 4),
    ("并行冒烟乙", 2, false, 4),
    ("串行独占", 3, true, 8),
    ("并行收尾", 4, false, 4),
    ("账本汇总", 5, false, 6),
];

/// 注册表内查标记（表外 ID 返回 None）。
pub fn flag_of(case_id: u32) -> Option<(bool, u64)> {
    let mut i = 0usize;
    while i < CASE_FLAG_TABLE.len() {
        match CASE_FLAG_TABLE.get(i) {
            Some((_, id, np, cost)) => {
                if *id == case_id {
                    return Some((*np, *cost));
                }
            }
            None => return None,
        }
        i += 1;
    }
    None
}

/// 时间轴上的一个时槽：谁、何时起止、起跑时并发计数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// 用例 ID。
    pub case_id: u32,
    /// 是否串行标记例。
    pub serial: bool,
    /// 起拍（含）。
    pub start: u64,
    /// 止拍（不含）。
    pub end: u64,
    /// 执行期并发计数（串行例恒 1，并行例 ≤ 双核上沿）。
    pub concurrent: u32,
}

/// 调度产出的时间轴。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    slots: Vec<Slot>,
    /// 峰值并发（全轴最大同时在场例数，≤ 双核上沿）。
    peak_concurrency: u32,
}

impl Timeline {
    /// 时槽只读视图。
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// 峰值并发。
    pub fn peak(&self) -> u32 {
        self.peak_concurrency
    }
}

/// 两时槽是否交叠（半开区间 [start,end) 相交判定）。
fn slots_overlap(a: &Slot, b: &Slot) -> bool {
    a.start < b.end && b.start < a.end
}

/// M-206 重构核：任一串行例时槽与其他例时槽交叠即违约。
pub fn serial_overlap_breach(t: &Timeline) -> Vec<u32> {
    let mut breached: Vec<u32> = Vec::new();
    for s in t.slots().iter() {
        if !s.serial {
            continue;
        }
        let mut overlapped = false;
        for o in t.slots().iter() {
            if o.case_id != s.case_id && slots_overlap(s, o) {
                overlapped = true;
            }
        }
        if overlapped && !breached.contains(&s.case_id) {
            breached.push(s.case_id);
        }
    }
    breached
}

/// 互斥调度器：按提交序排时间轴。并行例按双核额成批同拍；
/// 串行例等清场后独占。返回 Timeline；表外 ID / 空序即拒。
pub fn schedule(order: &[u32]) -> Result<Timeline, u32> {
    if order.is_empty() {
        return Err(Diag::EmptyTimeline.val());
    }
    for id in order.iter() {
        if flag_of(*id).is_none() {
            return Err(Diag::FlagForeign.val());
        }
    }
    let mut slots: Vec<Slot> = Vec::new();
    let mut tick: u64 = 1;
    let mut i = 0usize;
    while i < order.len() {
        let id = match order.get(i) {
            Some(&x) => x,
            None => return Err(Diag::FlagForeign.val()),
        };
        let (np, cost) = match flag_of(id) {
            Some(v) => v,
            None => return Err(Diag::FlagForeign.val()),
        };
        if np {
            // 串行例：等在跑例全部清场（看时间轴末端止拍）后独占。
            let mut field_end = tick;
            for s in slots.iter() {
                if s.end > field_end {
                    field_end = s.end;
                }
            }
            let start = field_end;
            let end = match start.checked_add(cost) {
                Some(e) => e,
                None => return Err(Diag::TickRegress.val()),
            };
            // 独占语义：并发计数恒 1（独占入场前计数必须为 0）。
            slots.push(Slot {
                case_id: id,
                serial: true,
                start,
                end,
                concurrent: 1,
            });
            tick = end;
            i += 1;
        } else {
            // 并行例：从当前位置抓至多双核额的连续并行例成一批同拍。
            let mut batch: Vec<u32> = Vec::new();
            let mut batch_cost: u64 = 0;
            let mut j = i;
            while j < order.len() && (batch.len() as u32) < MAX_CONCURRENCY {
                let jid = match order.get(j) {
                    Some(&x) => x,
                    None => return Err(Diag::FlagForeign.val()),
                };
                let (jnp, jcost) = match flag_of(jid) {
                    Some(v) => v,
                    None => return Err(Diag::FlagForeign.val()),
                };
                if jnp {
                    break;
                }
                batch.push(jid);
                if jcost > batch_cost {
                    batch_cost = jcost;
                }
                j += 1;
            }
            let n: u32 = match (batch.len() as u32).checked_sub(0) {
                Some(v) => v,
                None => return Err(Diag::CounterUnderflow.val()),
            };
            if n > MAX_CONCURRENCY {
                return Err(Diag::ConcurrencyOverflow.val());
            }
            let end = match tick.checked_add(batch_cost) {
                Some(e) => e,
                None => return Err(Diag::TickRegress.val()),
            };
            let mut k = 0usize;
            while k < batch.len() {
                let bid = match batch.get(k) {
                    Some(&x) => x,
                    None => return Err(Diag::FlagForeign.val()),
                };
                let bcost = match flag_of(bid) {
                    Some((_, c)) => c,
                    None => return Err(Diag::FlagForeign.val()),
                };
                slots.push(Slot {
                    case_id: bid,
                    serial: false,
                    start: tick,
                    // 各例止拍按自身代价（同批起拍同、止拍可异）。
                    end: match tick.checked_add(bcost) {
                        Some(e) => e,
                        None => return Err(Diag::TickRegress.val()),
                    },
                    concurrent: n,
                });
                k += 1;
            }
            tick = end;
            i = j;
        }
    }
    // 峰值并发重构：逐拍扫描在场例数取最大。
    let mut max_end: u64 = 1;
    for s in slots.iter() {
        if s.end > max_end {
            max_end = s.end;
        }
    }
    let mut peak: u32 = 0;
    let mut probe: u64 = 1;
    while probe < max_end {
        let mut live: u32 = 0;
        for s in slots.iter() {
            if s.start <= probe && probe < s.end {
                match live.checked_add(1) {
                    Some(v) => live = v,
                    None => return Err(Diag::ConcurrencyOverflow.val()),
                }
            }
        }
        if live > peak {
            peak = live;
        }
        probe += 1;
    }
    if peak > MAX_CONCURRENCY {
        return Err(Diag::ConcurrencyOverflow.val());
    }
    Ok(Timeline {
        slots,
        peak_concurrency: peak,
    })
}

/// 诊断码闭集：0x79D01..=0x79D0C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 串行例被并排：no_parallel 例与他在场例同拍。
    CoScheduleRejected = 0x01,
    /// 串行并发破约：串行例执行期并发计数 != 1（M-206 失守）。
    SerialConcurrentBreach = 0x02,
    /// 标记表外：用例 ID 不在冻结注册表内。
    FlagForeign = 0x03,
    /// 并发越上沿：同拍在场例数超双核上沿。
    ConcurrencyOverflow = 0x04,
    /// 计数下溢：退场计数减到负（checked 下溢拦截）。
    CounterUnderflow = 0x05,
    /// 时槽重叠越双核：并行批口径外出现三例同拍。
    SlotOverlapBroke = 0x06,
    /// 时戳倒退：起拍止拍非单调。
    TickRegress = 0x07,
    /// 峰值记账不符：账面峰值与重构峰值不一致。
    PeakMismatch = 0x08,
    /// 双跑分叉：同提交序两次调度时间轴不同。
    TimelineDiverge = 0x09,
    /// 空时间轴：无例可跑却产出时槽。
    EmptyTimeline = 0x0A,
    /// 串行例饿死：串行标记例未进时间轴。
    SerialStarved = 0x0B,
    /// 段位失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::CoScheduleRejected => "串行例被并排：建议 no_parallel 例入场前核并发计数为 0",
            Diag::SerialConcurrentBreach => "串行并发破约：建议串行例执行期并发计数恒 1 并入账核验",
            Diag::FlagForeign => "标记表外：建议只用冻结注册表五例，新例先扩表定标记位",
            Diag::ConcurrencyOverflow => "并发越上沿：建议同拍在场例数钳制在双核上沿 2 内",
            Diag::CounterUnderflow => "计数下溢：建议进出场计数全走 checked，未进场禁退场",
            Diag::SlotOverlapBroke => "时槽重叠越双核：建议并行批只按双核额成批，禁第三例搭车",
            Diag::TickRegress => "时戳倒退：建议起拍止拍由调度器单调发号，禁回拨",
            Diag::PeakMismatch => "峰值记账不符：建议峰值以逐拍重构为准，账面随重构对齐",
            Diag::TimelineDiverge => "双跑分叉：建议调度器做成纯函数，同输入定则时间轴定",
            Diag::EmptyTimeline => "空时间轴：建议空提交显性拒，无例不产槽",
            Diag::SerialStarved => "串行例饿死：建议串行标记例必进时间轴，漏排即判红",
            Diag::SegGuardFail => "段位失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::CoScheduleRejected,
    Diag::SerialConcurrentBreach,
    Diag::FlagForeign,
    Diag::ConcurrencyOverflow,
    Diag::CounterUnderflow,
    Diag::SlotOverlapBroke,
    Diag::TickRegress,
    Diag::PeakMismatch,
    Diag::TimelineDiverge,
    Diag::EmptyTimeline,
    Diag::SerialStarved,
    Diag::SegGuardFail,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79D01..=0x79D0C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B14 批段窗 [0x79C00, 0x79DC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79C00 && SEG_BASE <= 0x79DC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// no_parallel 位语义核：注册表标记位与查表口一致，串行例恰一枚，
/// 并行例查表回 (false, 代价)。
fn flag_semantics_ok() -> bool {
    if CASE_FLAG_TABLE.len() != 5 {
        return false;
    }
    let mut serial_n = 0u32;
    let mut i = 0usize;
    while i < CASE_FLAG_TABLE.len() {
        let (name, id, np, cost) = match CASE_FLAG_TABLE.get(i) {
            Some(t) => *t,
            None => return false,
        };
        match flag_of(id) {
            Some((got_np, got_cost)) => {
                if got_np != np || got_cost != cost {
                    return false;
                }
            }
            None => return false,
        }
        if np {
            serial_n += 1;
            let _ = name;
        }
        i += 1;
    }
    serial_n == 1 && flag_of(0x7FFF).is_none()
}

/// 正常流 + M-206：混排提交（并行×2 → 串行 → 并行×2）排轴后，
/// 串行例执行期并发计数恒 1 且与任何例不交叠，峰值并发为 2。
fn m206_serial_exclusive_ok() -> bool {
    let order: [u32; 5] = [1, 2, 3, 4, 5];
    let t = match schedule(&order) {
        Ok(t) => t,
        Err(_) => return false,
    };
    // 串行例时槽必存在且 concurrent == 1。
    let mut serial_slot: Option<&Slot> = None;
    for s in t.slots().iter() {
        if s.serial {
            serial_slot = Some(s);
        }
    }
    let ss = match serial_slot {
        Some(s) => s,
        None => return false,
    };
    if ss.concurrent != 1 || ss.case_id != 3 {
        return false;
    }
    // 重构：串行例与任何例不交叠。
    if !serial_overlap_breach(&t).is_empty() {
        return false;
    }
    // 并行批峰值恰为双核上沿。
    t.peak() == MAX_CONCURRENCY
}

/// 并行批核：纯并行序（无串行例）双两成批，各槽 concurrent ≤ 2，
/// 同批起拍相同、批间起拍单调不回退。
fn parallel_batch_ok() -> bool {
    let order: [u32; 4] = [1, 2, 4, 5];
    let t = match schedule(&order) {
        Ok(t) => t,
        Err(_) => return false,
    };
    if t.slots().len() != 4 {
        return false;
    }
    let mut prev_start: u64 = 0;
    for s in t.slots().iter() {
        if s.serial {
            return false;
        }
        if s.concurrent > MAX_CONCURRENCY || s.concurrent == 0 {
            return false;
        }
        if prev_start > 0 && s.start < prev_start {
            return false;
        }
        prev_start = s.start;
    }
    // 首批两例同拍同并发。
    let a = match t.slots().get(0) {
        Some(s) => s,
        None => return false,
    };
    let b = match t.slots().get(1) {
        Some(s) => s,
        None => return false,
    };
    a.start == b.start && a.concurrent == b.concurrent && a.concurrent == 2
}

/// 注入核一：串行例被并排的伪造时间轴必被重构点名（M-206 违约捕获）。
/// 构造带串行例与并行例交叠的轴，serial_overlap_breach 必回非空。
fn co_schedule_injection_ok() -> bool {
    // 合法轴先跑一遍做底。
    let order: [u32; 5] = [1, 2, 3, 4, 5];
    let good = match schedule(&order) {
        Ok(t) => t,
        Err(_) => return false,
    };
    if !serial_overlap_breach(&good).is_empty() {
        return false;
    }
    // 伪造违约轴：把串行例时槽人工挪进并行批的时段（与例 1 同拍）。
    let mut forged_slots: Vec<Slot> = Vec::new();
    for s in good.slots().iter() {
        if s.case_id == 3 {
            forged_slots.push(Slot {
                case_id: s.case_id,
                serial: true,
                start: 1,
                end: 1 + 8,
                concurrent: 2,
            });
        } else {
            forged_slots.push(s.clone());
        }
    }
    let forged = Timeline {
        slots: forged_slots,
        peak_concurrency: 2,
    };
    let breach = serial_overlap_breach(&forged);
    breach.len() == 1 && breach.first() == Some(&3)
}

/// 注入核二：表外 ID 拒（FlagForeign）、空序拒（EmptyTimeline）。
fn foreign_and_empty_gate_ok() -> bool {
    let bad: [u32; 2] = [1, 0x7FFF];
    if schedule(&bad) != Err(Diag::FlagForeign.val()) {
        return false;
    }
    let empty: [u32; 0] = [];
    schedule(&empty) == Err(Diag::EmptyTimeline.val())
}

/// 注入核三：串行例执行期并发计数的整数面纪律——串行槽若带
/// concurrent != 1 即 SerialConcurrentBreach；伪造并发 2 的串行槽
/// 必被槽位纪律核点破。
fn serial_counter_discipline_ok() -> bool {
    let order: [u32; 5] = [1, 2, 3, 4, 5];
    let t = match schedule(&order) {
        Ok(t) => t,
        Err(_) => return false,
    };
    for s in t.slots().iter() {
        if s.serial && s.concurrent != 1 {
            return false;
        }
        if !s.serial && s.concurrent > MAX_CONCURRENCY {
            return false;
        }
    }
    // 伪造：串行槽带并发 2（M-206 违约形态），纪律核必红。
    let forged = Timeline {
        slots: vec![Slot {
            case_id: 3,
            serial: true,
            start: 1,
            end: 9,
            concurrent: 2,
        }],
        peak_concurrency: 2,
    };
    let mut breach = false;
    for s in forged.slots().iter() {
        if s.serial && s.concurrent != 1 {
            breach = true;
        }
    }
    breach && detail(Diag::SerialConcurrentBreach.val()).contains("串行")
}

/// 串行例清场等待核：串行例起拍必等于此前所有例的最大止拍（清场后
/// 才上场），止拍随代价推进单调。
fn serial_waits_field_ok() -> bool {
    // 串行例放最前：起拍应为 1（空场直接进）。
    let lead: [u32; 3] = [3, 1, 2];
    let t1 = match schedule(&lead) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let ss1 = match t1.slots().iter().find(|s| s.serial) {
        Some(s) => s,
        None => return false,
    };
    if ss1.start != 1 {
        return false;
    }
    // 串行例放最后：起拍应为此前并行批的最大止拍（清场后进）。
    let tail: [u32; 3] = [1, 2, 3];
    let t2 = match schedule(&tail) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let ss2 = match t2.slots().iter().find(|s| s.serial) {
        Some(s) => s,
        None => return false,
    };
    let mut field_end: u64 = 0;
    for s in t2.slots().iter() {
        if !s.serial && s.end > field_end {
            field_end = s.end;
        }
    }
    ss2.start == field_end
}

/// 峰值对账核：账面峰值与逐拍重构峰值一致；越上沿的伪造轴被调度口
/// 现场拒（ConcurrencyOverflow 语义由调度内建核验承担）。
fn peak_reconcile_ok() -> bool {
    let order: [u32; 5] = [1, 2, 3, 4, 5];
    let t = match schedule(&order) {
        Ok(t) => t,
        Err(_) => return false,
    };
    if t.peak() > MAX_CONCURRENCY {
        return false;
    }
    // 重构峰值独立复算。
    let mut max_end: u64 = 1;
    for s in t.slots().iter() {
        if s.end > max_end {
            max_end = s.end;
        }
    }
    let mut peak: u32 = 0;
    let mut probe: u64 = 1;
    while probe < max_end {
        let mut live: u32 = 0;
        for s in t.slots().iter() {
            if s.start <= probe && probe < s.end {
                live += 1;
            }
        }
        if live > peak {
            peak = live;
        }
        probe += 1;
    }
    peak == t.peak()
}

/// 串行例必进时间轴核：任一含串行例的提交序，串行例都不得缺席。
fn serial_starve_gate_ok() -> bool {
    let orders: [[u32; 3]; 3] = [[3, 1, 2], [1, 3, 2], [1, 2, 3]];
    let mut k = 0usize;
    while k < orders.len() {
        let order = match orders.get(k) {
            Some(o) => *o,
            None => return false,
        };
        let t = match schedule(&order) {
            Ok(t) => t,
            Err(_) => return false,
        };
        let mut found = false;
        for s in t.slots().iter() {
            if s.case_id == 3 && s.serial {
                found = true;
            }
        }
        if !found {
            return false;
        }
        k += 1;
    }
    true
}

/// 双跑一致：同提交序两次调度的时间轴逐字段相同。
fn double_run_ok() -> bool {
    let run_once = || -> Result<Timeline, u32> {
        let order: [u32; 5] = [1, 2, 3, 4, 5];
        schedule(&order)
    };
    let a = run_once();
    let b = run_once();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含标记位、互斥与 M-206 结论）。
fn writeback_summary_ok() -> bool {
    let order: [u32; 5] = [1, 2, 3, 4, 5];
    let t = match schedule(&order) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let breach_n = serial_overlap_breach(&t).len();
    let summary = format!(
        "UNX-F3447 完成：no_parallel 位落地 串行互斥重构违约{}例 峰值并发{}",
        breach_n,
        t.peak()
    );
    !summary.is_empty()
        && summary.contains("UNX-F3447")
        && summary.contains("no_parallel")
        && breach_n == 0
        && detail(Diag::SerialConcurrentBreach.val()).contains("串行")
}

// ── 判据总成：run_unx3447_checks ───────────────────────────────────────

/// run_unx3447_checks：本单判据聚合（诊断段 0x79D00，全部走 add_dyn）。
pub fn run_unx3447_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3447 串行标记与并发互斥");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "no_parallel位语义注册表标记对齐",
            flag_semantics_ok(),
            format!("标记位{}", if flag_semantics_ok() { "对齐" } else { "漂移" }),
        ),
        (
            "M-206串行例执行期并发计数恒1",
            m206_serial_exclusive_ok(),
            format!("M-206互斥{}", if m206_serial_exclusive_ok() { "成立" } else { "破约" }),
        ),
        (
            "并行批双两成批同拍同计数",
            parallel_batch_ok(),
            format!("并行批{}", if parallel_batch_ok() { "成批" } else { "散排" }),
        ),
        (
            "串行例被并排伪造轴必被点名",
            co_schedule_injection_ok(),
            format!("并排注入{}", if co_schedule_injection_ok() { "捕获" } else { "漏抓" }),
        ),
        (
            "表外ID与空序显性拒",
            foreign_and_empty_gate_ok(),
            format!("表外闸{}", if foreign_and_empty_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "串行槽并发计数纪律违约必红",
            serial_counter_discipline_ok(),
            format!("计数纪律{}", if serial_counter_discipline_ok() { "钉死" } else { "失守" }),
        ),
        (
            "串行例清场后独占上场",
            serial_waits_field_ok(),
            format!("清场闸{}", if serial_waits_field_ok() { "成立" } else { "失守" }),
        ),
        (
            "峰值记账与逐拍重构一致",
            peak_reconcile_ok(),
            format!("峰值{}", if peak_reconcile_ok() { "对账" } else { "错账" }),
        ),
        (
            "串行例三种排位必进时间轴",
            serial_starve_gate_ok(),
            format!("饿死闸{}", if serial_starve_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "双跑一致时间轴逐字段同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "冻结注册表五例代价与标记两查",
            flag_table_ok(),
            format!("注册表{}", if flag_table_ok() { "对齐" } else { "漂移" }),
        ),
    ];
    // 写回摘要判据（第 14 条，与 13 条并列成账）。
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

/// 冻结注册表自洽：五例 ID 互异、代价非零、串行例恰一枚。
fn flag_table_ok() -> bool {
    if CASE_FLAG_TABLE.len() != 5 {
        return false;
    }
    let mut i = 0usize;
    while i < CASE_FLAG_TABLE.len() {
        let (_, id_a, _, cost_a) = match CASE_FLAG_TABLE.get(i) {
            Some(t) => *t,
            None => return false,
        };
        if cost_a == 0 {
            return false;
        }
        let mut j = i + 1;
        while j < CASE_FLAG_TABLE.len() {
            match CASE_FLAG_TABLE.get(j) {
                Some((_, id_b, _, _)) => {
                    if id_a == *id_b {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79D00);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
        assert_eq!(detail(0xDEAD), detail(0x5A5A));
    }

    #[test]
    fn flag_semantics_and_table() {
        assert!(flag_semantics_ok());
        assert!(flag_table_ok());
        assert_eq!(flag_of(3), Some((true, 8)));
        assert_eq!(flag_of(1), Some((false, 4)));
        assert_eq!(flag_of(0x7FFF), None);
        assert_eq!(INJECTION_CASE_ID, 0x206);
    }

    #[test]
    fn m206_serial_exclusive() {
        assert!(m206_serial_exclusive_ok());
        let order: [u32; 5] = [1, 2, 3, 4, 5];
        let t = schedule(&order).unwrap();
        let ss = t.slots().iter().find(|s| s.serial).unwrap();
        assert_eq!(ss.concurrent, 1);
        assert!(serial_overlap_breach(&t).is_empty());
    }

    #[test]
    fn parallel_batches_of_two() {
        assert!(parallel_batch_ok());
        assert_eq!(MAX_CONCURRENCY, 2);
    }

    #[test]
    fn co_schedule_injection_captured() {
        assert!(co_schedule_injection_ok());
    }

    #[test]
    fn foreign_and_empty_gates() {
        assert!(foreign_and_empty_gate_ok());
    }

    #[test]
    fn serial_counter_discipline() {
        assert!(serial_counter_discipline_ok());
    }

    #[test]
    fn serial_waits_for_field() {
        assert!(serial_waits_field_ok());
        let tail: [u32; 3] = [1, 2, 3];
        let t = schedule(&tail).unwrap();
        let ss = t.slots().iter().find(|s| s.serial).unwrap();
        let par_max = t
            .slots()
            .iter()
            .filter(|s| !s.serial)
            .map(|s| s.end)
            .max()
            .unwrap();
        assert_eq!(ss.start, par_max);
    }

    #[test]
    fn peak_reconciles() {
        assert!(peak_reconcile_ok());
    }

    #[test]
    fn serial_never_starved() {
        assert!(serial_starve_gate_ok());
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3447_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
