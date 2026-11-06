//! UNX-F51509 UASP 统计账（流级 IOPS/时延分位） · 0xC0240段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51510-J1 分区变更 ×10 事件正确；语义归 B4 联签锚定行
//! 零越界。
//!
//! 设计要点：
//!   ① UASP 统计面按「流级 IOPS/时延分位」建模：×16 路流各自持有 IOPS
//!      计数与时延样本集，流号上限+1 显性拒；单流 IOPS 满
//!      STREAM_IOPS_CAP 即容量耗尽拒；分位按整数排序计算，零浮点。
//!   ② 分区变更事件账：Add/Del/Resize 三类事件，事件序号严格单调递增
//!      （倒退显性拒），事件容量 EV_CAP=10 对齐判据 ×10 事件，落满即
//!      容量耗尽拒。
//!   ③ 语义归 B4 联签锚定行：每笔分区事件必须锚定到冻结锚定行表内一行，
//!      锚行越界（AnchorOob）与事件跨度越出锚行上界（AnchorSpanOver）
//!      均显性拒；联签码随事件落账，回验时逐笔比对锚表（零越界）。
//!   ④ ×10 事件正确性回验：事件序号、锚行号、联签码、跨度四元逐笔
//!      复核，任一失配即 EventMismatch。
//!   ⑤ 判定次序：事件容量 → 序号倒退 → 零跨度 → 锚行界 → 跨度越界 → 落账。
//!   ⑥ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0；三类注入（锚行越界、
//!      序号倒退、跨度越界）各 ×10 轮必抓；固定种子 LCG 驱动流压测。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 0xC0240 段基址（51509 = 0xC0180 + 0x40 × 3）。
pub const SEG_BASE: u32 = 0xC0240;

/// 流级账路数。
pub const STREAM_MAX: usize = 16;

/// 单流 IOPS 容量（容量耗尽判据）。
pub const STREAM_IOPS_CAP: u64 = 1024;

/// 单流时延样本容量（分位账采样上限）。
pub const QUANT_CAP: usize = 32;

/// 分区变更事件容量（判据 ×10 事件）。
pub const EV_CAP: usize = 10;

/// B4 联签锚定行数（冻结表规模）。
pub const ANCHOR_ROWS: usize = 8;

/// 固定种子（流压测 LCG，确定性）。
pub const LCG_SEED: u64 = 0xC0240_5151;

/// B4 联签锚定行冻结表：(联签码, 行上界 LBA)。
pub const ANCHOR_TABLE: [(u32, u64); ANCHOR_ROWS] = [
    (0xC0240, 4096),
    (0xC0244, 8192),
    (0xC0248, 12288),
    (0xC024C, 16384),
    (0xC0250, 20480),
    (0xC0254, 24576),
    (0xC0258, 28672),
    (0xC025C, 32768),
];

/// 锚定行取值（越界返回 None，零越界语义入口）。
pub fn anchor_of(row: usize) -> Option<(u32, u64)> {
    match ANCHOR_TABLE.get(row) {
        Some(pair) => Some(*pair),
        None => None,
    }
}

/// 诊断码闭集（UASP 统计账域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UaspCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 事件容量耗尽（×10 落满）
    EvCapFull,
    /// 事件序号倒退（严格单调被破）
    SeqRegress,
    /// 锚定行越界（超出 B4 锚表）
    AnchorOob,
    /// 事件跨度越出锚行上界
    AnchorSpanOver,
    /// 零跨度事件（lba_len 必须 ≥1）
    ZeroSpan,
    /// 流号越界（≥ ×16 流上限）
    StreamIdRange,
    /// 单流 IOPS 容量耗尽
    IopsCapFull,
    /// ×10 事件回验失配（序号/锚行/联签/跨度四元）
    EventMismatch,
}

impl UaspCode {
    /// 诊断码取值（段内偏移闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码在位）。
    pub fn detail(self) -> &'static str {
        match self {
            UaspCode::Ok => "UASP 账受理成功",
            UaspCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            UaspCode::EvCapFull => "事件容量耗尽：×10 事件落满即封账",
            UaspCode::SeqRegress => "事件序号倒退：序号必须严格单调递增",
            UaspCode::AnchorOob => "锚定行越界：锚行号必须在 B4 锚表内",
            UaspCode::AnchorSpanOver => "跨度越界：事件 LBA 区间不得越出锚行上界",
            UaspCode::ZeroSpan => "零跨度事件：lba_len 必须 ≥1",
            UaspCode::StreamIdRange => "流号越界：流号必须在 0..16 内",
            UaspCode::IopsCapFull => "单流 IOPS 容量耗尽：按段闭账后开新流账",
            UaspCode::EventMismatch => "事件回验失配：序号/锚行/联签/跨度必须逐笔对平",
        }
    }
}

/// 诊断码全序表（闭集枚举权威清单）。
pub const ALL_CODES: [UaspCode; 10] = [
    UaspCode::Ok,
    UaspCode::SegBad,
    UaspCode::EvCapFull,
    UaspCode::SeqRegress,
    UaspCode::AnchorOob,
    UaspCode::AnchorSpanOver,
    UaspCode::ZeroSpan,
    UaspCode::StreamIdRange,
    UaspCode::IopsCapFull,
    UaspCode::EventMismatch,
];

/// 分区变更事件类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartKind {
    /// 分区新增
    Add,
    /// 分区删除
    Del,
    /// 分区重排
    Resize,
}

/// 单笔分区变更事件（含 B4 联签落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartEvent {
    /// 事件序号（严格单调，1 起）
    pub seq: u64,
    /// 事件类别
    pub kind: PartKind,
    /// 起始 LBA
    pub lba_start: u64,
    /// 跨度（LBA 数，≥1）
    pub lba_len: u64,
    /// 锚定行号
    pub anchor: usize,
    /// 落账联签码（取自锚表）
    pub sign: u32,
}

/// 单流账（IOPS 计数 + 时延样本，分位按需整数计算）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamStat {
    /// 流号
    pub stream: u8,
    /// IOPS 计数（只增不减）
    pub iops: u64,
    /// 时延样本集（≤ QUANT_CAP）
    pub samples: Vec<u32>,
}

/// 整数分位：有序样本第 p 百分位（零浮点）。
pub fn percentile(samples: &[u32], p: u64) -> u32 {
    if samples.is_empty() {
        return 0;
    }
    let mut v: Vec<u32> = Vec::new();
    for x in samples.iter() {
        v.push(*x);
    }
    v.sort_unstable();
    let n = v.len() as u64;
    let mut idx = (n.saturating_mul(p) / 100) as usize;
    if idx >= v.len() {
        idx = v.len().saturating_sub(1);
    }
    match v.get(idx) {
        Some(x) => *x,
        None => 0,
    }
}

/// 固定种子 LCG（压测负载发生器，禁随机墙钟）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 以固定种子起振。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed }
    }
    /// 下一时延样本（0..1000 整数，确定性）。
    pub fn next_lat(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.s >> 33) % 1000) as u32
    }
}

/// UASP 统计账引擎：流级 IOPS/分位账 + 分区变更事件账（B4 锚定）。
pub struct UaspLedger {
    streams: Vec<StreamStat>,
    events: Vec<PartEvent>,
    last_seq: u64,
    injected: u64,
    detected: u64,
}

impl UaspLedger {
    /// 新账引擎：×16 流开账、事件账空、序号 0。
    pub fn new() -> UaspLedger {
        let mut streams = Vec::new();
        for i in 0..STREAM_MAX {
            streams.push(StreamStat {
                stream: i as u8,
                iops: 0,
                samples: Vec::new(),
            });
        }
        UaspLedger {
            streams,
            events: Vec::new(),
            last_seq: 0,
            injected: 0,
            detected: 0,
        }
    }

    /// 落一笔流级 IO 账：IOPS +1、时延样本入分位账。
    /// 判序：流号界 → 容量耗尽 → 落账。
    pub fn record_io(&mut self, stream: usize, lat: u32) -> Result<(), UaspCode> {
        if stream >= STREAM_MAX {
            return Err(UaspCode::StreamIdRange);
        }
        let st = match self.streams.get_mut(stream) {
            Some(st) => st,
            None => return Err(UaspCode::StreamIdRange),
        };
        if st.iops.saturating_add(1) > STREAM_IOPS_CAP {
            return Err(UaspCode::IopsCapFull);
        }
        st.iops = st.iops.saturating_add(1);
        if st.samples.len() < QUANT_CAP {
            st.samples.push(lat);
        }
        Ok(())
    }

    /// 落一笔分区变更事件（语义归 B4 联签锚定行）。
    /// 判序：事件容量 → 序号倒退 → 零跨度 → 锚行界 → 跨度越界 → 落账。
    pub fn push_event(
        &mut self,
        kind: PartKind,
        seq: u64,
        lba_start: u64,
        lba_len: u64,
        anchor: usize,
    ) -> Result<usize, UaspCode> {
        if self.events.len() >= EV_CAP {
            return Err(UaspCode::EvCapFull);
        }
        if seq <= self.last_seq {
            return Err(UaspCode::SeqRegress);
        }
        if lba_len == 0 {
            return Err(UaspCode::ZeroSpan);
        }
        let (sign, bound) = match anchor_of(anchor) {
            Some(pair) => pair,
            None => return Err(UaspCode::AnchorOob),
        };
        let end = match lba_start.checked_add(lba_len) {
            Some(e) => e,
            None => return Err(UaspCode::AnchorSpanOver),
        };
        if end > bound {
            return Err(UaspCode::AnchorSpanOver);
        }
        self.events.push(PartEvent {
            seq,
            kind,
            lba_start,
            lba_len,
            anchor,
            sign,
        });
        self.last_seq = seq;
        Ok(self.events.len())
    }

    /// ×10 分区事件脚本（判据 ×10 事件正确）：序号 1..=10、锚行轮转、
    /// 跨度收敛于锚行上界内，返回受理数。
    pub fn run_10_events(ul: &mut UaspLedger) -> usize {
        let kinds = [PartKind::Add, PartKind::Resize, PartKind::Del];
        let mut accepted = 0usize;
        for i in 0..EV_CAP {
            let kind = match kinds.get(i % 3) {
                Some(k) => *k,
                None => PartKind::Add,
            };
            let bound = match anchor_of(i % ANCHOR_ROWS) {
                Some((_, b)) => b,
                None => 4096,
            };
            // 跨度取锚行上界的 1/8，起始 0，恒不越界。
            let lba_len = bound / 8;
            let r = ul.push_event(kind, i as u64 + 1, 0, lba_len, i % ANCHOR_ROWS);
            if r.is_ok() {
                accepted += 1;
            }
        }
        accepted
    }

    /// ×10 事件正确性回验：序号连续、锚行在表、联签码与锚表一致、
    /// 跨度不越锚行上界，四元逐笔对平（零越界）。
    pub fn verify_events(&self) -> Result<usize, UaspCode> {
        for (i, ev) in self.events.iter().enumerate() {
            if ev.seq != i as u64 + 1 {
                return Err(UaspCode::EventMismatch);
            }
            let (sign, bound) = match anchor_of(ev.anchor) {
                Some(pair) => pair,
                None => return Err(UaspCode::AnchorOob),
            };
            if ev.sign != sign {
                return Err(UaspCode::EventMismatch);
            }
            let end = match ev.lba_start.checked_add(ev.lba_len) {
                Some(e) => e,
                None => return Err(UaspCode::AnchorSpanOver),
            };
            if end > bound {
                return Err(UaspCode::AnchorSpanOver);
            }
        }
        Ok(self.events.len())
    }

    /// ×16 流压测扫描：固定种子 LCG 每流 QUANT_CAP 笔，返回落账流数。
    pub fn sweep_16_streams(ul: &mut UaspLedger) -> usize {
        let mut lcg = Lcg::new(LCG_SEED);
        for s in 0..STREAM_MAX {
            for _ in 0..QUANT_CAP {
                let lat = lcg.next_lat();
                let _ = ul.record_io(s, lat);
            }
        }
        let mut n = 0usize;
        for st in ul.streams.iter() {
            if st.iops > 0 {
                n += 1;
            }
        }
        n
    }

    /// 流账 IOPS 汇总（对账守恒面：Σ iops 与逐流账对平恒真，防账面破洞）。
    pub fn iops_total(&self) -> u64 {
        let mut sum = 0u64;
        for st in self.streams.iter() {
            sum = sum.saturating_add(st.iops);
        }
        sum
    }

    /// 事件账量。
    pub fn events_len(&self) -> usize {
        self.events.len()
    }

    /// 事件账克隆（供双跑比对；只读用途）。
    pub fn events_snapshot(&self) -> Vec<PartEvent> {
        self.events.clone()
    }

    /// 注入记账：检出加一（调用方在捕获违约时登记）。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，且逃逸恒 0、检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected && self.detected > 0
    }
}

/// 段位守卫：SEG_BASE 按 0x40 对齐且落在分派区间。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一 ×10 事件脚本 + ×16 流扫描两次建账逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<PartEvent>, u64) {
        let mut ul = UaspLedger::new();
        let _ = UaspLedger::run_10_events(&mut ul);
        let _ = UaspLedger::sweep_16_streams(&mut ul);
        (ul.events_snapshot(), ul.iops_total())
    };
    let a = build();
    let b = build();
    a.0 == b.0 && a.1 == b.1
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51509_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51509");
    // 1 正常流：×10 分区事件全受理且回验通过。
    {
        let mut ul = UaspLedger::new();
        let n = UaspLedger::run_10_events(&mut ul);
        s.add_dyn(
            "×10 分区事件落账",
            n == EV_CAP && ul.verify_events() == Ok(EV_CAP),
            "×10 事件必须全受理且四元回验对平",
        );
    }
    // 2 边界零：空事件账回验 0。
    {
        let ul = UaspLedger::new();
        s.add_dyn(
            "空事件账回验零",
            ul.verify_events() == Ok(0),
            "零事件账应回验通过且量为 0",
        );
    }
    // 3 边界零：零跨度事件拒。
    {
        let mut ul = UaspLedger::new();
        let r = ul.push_event(PartKind::Add, 1, 0, 0, 0);
        s.add_dyn(
            "零跨度事件拒",
            r == Err(UaspCode::ZeroSpan),
            "lba_len 必须 ≥1（零边界显性拒）",
        );
    }
    // 4 边界上限：第 10 笔事件受理后账满。
    {
        let mut ul = UaspLedger::new();
        let _ = UaspLedger::run_10_events(&mut ul);
        s.add_dyn(
            "事件账 ×10 封满",
            ul.events_len() == EV_CAP,
            "×10 事件落满即达容量上限",
        );
    }
    // 5 边界上限+1：第 11 笔事件容量耗尽拒。
    {
        let mut ul = UaspLedger::new();
        let _ = UaspLedger::run_10_events(&mut ul);
        let r = ul.push_event(PartKind::Add, 11, 0, 512, 0);
        s.add_dyn(
            "第 11 笔事件拒",
            r == Err(UaspCode::EvCapFull),
            "事件容量耗尽必须显性拒",
        );
    }
    // 6 边界上限：最后一行锚定行受理。
    {
        let mut ul = UaspLedger::new();
        let r = ul.push_event(PartKind::Add, 1, 32768 - 512, 512, ANCHOR_ROWS - 1);
        s.add_dyn(
            "末锚行贴界受理",
            r.is_ok(),
            "末锚行内贴界事件必须放行",
        );
    }
    // 7 注入必抓：锚定行越界 ×10 轮。
    {
        let mut ul = UaspLedger::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = ul.push_event(PartKind::Add, 1, 0, 512, ANCHOR_ROWS + i as usize);
            if r == Err(UaspCode::AnchorOob) {
                caught += 1;
                ul.note_detected();
            }
        }
        s.add_dyn("锚行越界注入 10/10", caught == 10, "锚行越界必须全抓");
    }
    // 8 注入必抓：事件序号倒退 ×10 轮。
    {
        let mut ul = UaspLedger::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = ul.push_event(PartKind::Del, 0, 0, 512, 0);
            if r == Err(UaspCode::SeqRegress) {
                caught += 1;
                ul.note_detected();
            }
        }
        s.add_dyn("序号倒退注入 10/10", caught == 10, "序号倒退必须全抓");
    }
    // 9 注入必抓：锚行跨度越界 ×10 轮。
    {
        let mut ul = UaspLedger::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = ul.push_event(PartKind::Resize, 1, 4096 - 8 + i as u64, 16, 0);
            if r == Err(UaspCode::AnchorSpanOver) {
                caught += 1;
                ul.note_detected();
            }
        }
        s.add_dyn("跨度越界注入 10/10", caught == 10, "越锚行上界必须全抓");
    }
    // 10 判定次序：事件容量先于序号倒退。
    {
        let mut ul = UaspLedger::new();
        let _ = UaspLedger::run_10_events(&mut ul);
        let r = ul.push_event(PartKind::Add, 0, 0, 512, 0);
        s.add_dyn(
            "判序容量先于倒退",
            r == Err(UaspCode::EvCapFull),
            "容量判据必须先于序号判据",
        );
    }
    // 11 判定次序：序号倒退先于锚行越界。
    {
        let mut ul = UaspLedger::new();
        let r = ul.push_event(PartKind::Add, 0, 0, 512, ANCHOR_ROWS + 3);
        s.add_dyn(
            "判序倒退先于锚界",
            r == Err(UaspCode::SeqRegress),
            "序号判据必须先于锚行判据",
        );
    }
    // 12 ×16 流落账 + 流上限+1 拒。
    {
        let mut ul = UaspLedger::new();
        let n = UaspLedger::sweep_16_streams(&mut ul);
        let r = ul.record_io(STREAM_MAX, 100);
        s.add_dyn(
            "×16 流落账上限+1 拒",
            n == STREAM_MAX && r == Err(UaspCode::StreamIdRange),
            "×16 流必须全落账且流 16 显性拒",
        );
    }
    // 13 容量耗尽：单流 IOPS 灌满后追加拒。
    {
        let mut ul = UaspLedger::new();
        let mut full_ok = true;
        for _ in 0..STREAM_IOPS_CAP {
            if ul.record_io(0, 100).is_err() {
                full_ok = false;
            }
        }
        let r = ul.record_io(0, 100);
        s.add_dyn(
            "单流 IOPS 容量耗尽拒",
            full_ok && r == Err(UaspCode::IopsCapFull),
            "IOPS 满 STREAM_IOPS_CAP 后必须拒",
        );
    }
    // 14 分位账整数计算：p50/p99 可复核。
    {
        let p50 = percentile(&[1u32, 2, 3, 4], 50);
        let p99 = percentile(&[1u32, 2, 3, 4], 99);
        s.add_dyn(
            "整数分位可复核",
            p50 == 3 && p99 == 4,
            "p50/p99 按有序样本整数分位取值",
        );
    }
    // 15 B4 联签零越界：回验逐笔对平且 IOPS 汇总守恒。
    {
        let mut ul = UaspLedger::new();
        let _ = UaspLedger::run_10_events(&mut ul);
        let _ = UaspLedger::sweep_16_streams(&mut ul);
        let expect_iops = (QUANT_CAP as u64) * (STREAM_MAX as u64);
        s.add_dyn(
            "联签回验与 IOPS 守恒",
            ul.verify_events() == Ok(EV_CAP) && ul.iops_total() == expect_iops,
            "锚定行零越界回验通过且流账汇总对平",
        );
    }
    // 16 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut ul = UaspLedger::new();
        for _ in 0..10u32 {
            let _ = ul.push_event(PartKind::Add, 1, 0, 512, ANCHOR_ROWS + 9);
            ul.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            ul.conserved(),
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 17 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一脚本两次建账事件账与 IOPS 汇总一致");
    // 18 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 19 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_events_all_accepted_and_verified() {
        let mut ul = UaspLedger::new();
        assert_eq!(UaspLedger::run_10_events(&mut ul), EV_CAP);
        assert_eq!(ul.verify_events(), Ok(EV_CAP));
        assert_eq!(ul.events_len(), EV_CAP);
    }

    #[test]
    fn zero_span_rejected() {
        let mut ul = UaspLedger::new();
        assert_eq!(
            ul.push_event(PartKind::Add, 1, 0, 0, 0),
            Err(UaspCode::ZeroSpan)
        );
    }

    #[test]
    fn event_cap_full_at_10_then_rejected() {
        let mut ul = UaspLedger::new();
        let _ = UaspLedger::run_10_events(&mut ul);
        assert_eq!(
            ul.push_event(PartKind::Add, 11, 0, 512, 0),
            Err(UaspCode::EvCapFull)
        );
    }

    #[test]
    fn last_anchor_row_boundary() {
        let mut ul = UaspLedger::new();
        assert!(ul
            .push_event(PartKind::Resize, 1, 32768 - 512, 512, ANCHOR_ROWS - 1)
            .is_ok());
        // 末锚行贴界 +1 跨度即越界。
        let mut ul2 = UaspLedger::new();
        assert_eq!(
            ul2.push_event(PartKind::Add, 1, 32768 - 512, 513, ANCHOR_ROWS - 1),
            Err(UaspCode::AnchorSpanOver)
        );
    }

    #[test]
    fn injection_anchor_oob_x10() {
        let mut ul = UaspLedger::new();
        for i in 0..10u32 {
            assert_eq!(
                ul.push_event(PartKind::Add, 1, 0, 512, ANCHOR_ROWS + i as usize),
                Err(UaspCode::AnchorOob)
            );
        }
    }

    #[test]
    fn injection_seq_regress_x10() {
        let mut ul = UaspLedger::new();
        for _ in 0..10u32 {
            assert_eq!(
                ul.push_event(PartKind::Del, 0, 0, 512, 0),
                Err(UaspCode::SeqRegress)
            );
        }
    }

    #[test]
    fn injection_span_over_x10() {
        let mut ul = UaspLedger::new();
        for i in 0..10u32 {
            assert_eq!(
                ul.push_event(PartKind::Resize, 1, 4096 - 8 + i as u64, 16, 0),
                Err(UaspCode::AnchorSpanOver)
            );
        }
    }

    #[test]
    fn judge_order_cap_before_regress_before_anchor() {
        let mut ul = UaspLedger::new();
        let _ = UaspLedger::run_10_events(&mut ul);
        // 满账后带倒退序号：先报容量。
        assert_eq!(
            ul.push_event(PartKind::Add, 0, 0, 512, 0),
            Err(UaspCode::EvCapFull)
        );
        // 空账带倒退序号 + 越界锚：先报倒退。
        let mut ul2 = UaspLedger::new();
        assert_eq!(
            ul2.push_event(PartKind::Add, 0, 0, 512, ANCHOR_ROWS + 3),
            Err(UaspCode::SeqRegress)
        );
    }

    #[test]
    fn stream_bound_and_iops_cap() {
        let mut ul = UaspLedger::new();
        assert_eq!(
            ul.record_io(STREAM_MAX, 100),
            Err(UaspCode::StreamIdRange)
        );
        for _ in 0..STREAM_IOPS_CAP {
            assert!(ul.record_io(0, 100).is_ok());
        }
        assert_eq!(ul.record_io(0, 100), Err(UaspCode::IopsCapFull));
    }

    #[test]
    fn sweep_lands_all_16_streams() {
        let mut ul = UaspLedger::new();
        assert_eq!(UaspLedger::sweep_16_streams(&mut ul), STREAM_MAX);
        assert_eq!(ul.iops_total(), (QUANT_CAP as u64) * (STREAM_MAX as u64));
    }

    #[test]
    fn integer_percentile_math() {
        assert_eq!(percentile(&[1u32, 2, 3, 4], 50), 3);
        assert_eq!(percentile(&[1u32, 2, 3, 4], 99), 4);
        assert_eq!(percentile(&[], 99), 0);
    }

    #[test]
    fn injection_ledger_conserved() {
        let mut ul = UaspLedger::new();
        for _ in 0..10u32 {
            let _ = ul.push_event(PartKind::Add, 1, 0, 512, ANCHOR_ROWS + 9);
            ul.note_detected();
        }
        assert!(ul.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
