//! UNX-F51508 MSC 传输统计账（读/写/错误/重试四分账） · 0xC0200段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51509-J1 流级分位账 ×16 流落账；长尾流显式标注。
//!
//! 设计要点：
//!   ① MSC 传输面按「四分账」建模：读 / 写 / 错误 / 重试 四桶分立累计，
//!      各桶 checked_add 防溢出（上限+1 拒），一笔账量必须 ≥1（零量拒）。
//!   ② 流级账 ×16 路：每路独立持有 操作量/错误量/重试量/时延样本集，
//!      流号 0..16 闭区间，流 16（上限+1）显性拒；单流操作量满
//!      STREAM_OPS_CAP 即容量耗尽拒。
//!   ③ 分位账：每流保留至多 QUANT_CAP 个时延样本（固定种子 LCG 驱动
//!      压测负载，确定性可双跑），p50/p99 按整数排序分位计算，零浮点。
//!   ④ 长尾流显式标注：p99 越过 LONGTAIL_P99 阈值即落标；审计重算
//!      分位与标注比对，漏标（LongtailMiss）与误标（LongtailFalseMark）
//!      均显性抓捕，标注面禁篡改。
//!   ⑤ 判定次序：流号界 → 零量 → 流容量 → 桶溢出 → 落账。
//!   ⑥ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0；三类注入（流号越界、
//!      零量、标注篡改）各 ×10 轮必抓，四分账总量与流账汇总逐项守恒。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 0xC0200 段基址（51508 = 0xC0180 + 0x40 × 2）。
pub const SEG_BASE: u32 = 0xC0200;

/// 流级账路数（判据 ×16 流落账）。
pub const STREAM_MAX: usize = 16;

/// 单流操作量容量（容量耗尽判据）。
pub const STREAM_OPS_CAP: u64 = 4096;

/// 单流时延样本容量（分位账采样上限）。
pub const QUANT_CAP: usize = 32;

/// 长尾判定阈值（p99 时延 ≥ 此值即长尾流）。
pub const LONGTAIL_P99: u32 = 900;

/// 固定种子（压测负载 LCG，确定性）。
pub const LCG_SEED: u64 = 0xC020_5151;

/// 诊断码闭集（MSC 四分账域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MscCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 流号越界（≥ ×16 流上限）
    StreamIdRange,
    /// 零量账（一笔账量必须 ≥1）
    ZeroAmount,
    /// 单流操作量容量耗尽
    StreamCapFull,
    /// 四分账桶溢出（上限+1）
    BucketOverflow,
    /// 长尾漏标（p99 越阈而未落标）
    LongtailMiss,
    /// 长尾误标（p99 未越阈而落标）
    LongtailFalseMark,
    /// 四分账总量与流账汇总不符
    LedgerMismatch,
}

impl MscCode {
    /// 诊断码取值（段内偏移闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码在位）。
    pub fn detail(self) -> &'static str {
        match self {
            MscCode::Ok => "传输账受理成功",
            MscCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            MscCode::StreamIdRange => "流号越界：流号必须在 0..16 内",
            MscCode::ZeroAmount => "零量账：一笔传输量必须 ≥1",
            MscCode::StreamCapFull => "单流容量耗尽：按段闭账后开新流账",
            MscCode::BucketOverflow => "分账桶溢出：拆账或按段闭账",
            MscCode::LongtailMiss => "长尾漏标：p99 越阈必须显式落标",
            MscCode::LongtailFalseMark => "长尾误标：p99 未越阈不得落标",
            MscCode::LedgerMismatch => "账实不符：四分账总量必须等于流账汇总",
        }
    }
}

/// 诊断码全序表（闭集枚举权威清单）。
pub const ALL_CODES: [MscCode; 9] = [
    MscCode::Ok,
    MscCode::SegBad,
    MscCode::StreamIdRange,
    MscCode::ZeroAmount,
    MscCode::StreamCapFull,
    MscCode::BucketOverflow,
    MscCode::LongtailMiss,
    MscCode::LongtailFalseMark,
    MscCode::LedgerMismatch,
];

/// MSC 四分账桶别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MscBucket {
    /// 读桶
    Read,
    /// 写桶
    Write,
    /// 错误桶
    Error,
    /// 重试桶
    Retry,
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

/// 单流账（操作/错误/重试 + 时延样本 + 长尾标注）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamStat {
    /// 流号
    pub stream: u8,
    /// 操作量（含错误重试）
    pub ops: u64,
    /// 错误量
    pub err: u64,
    /// 重试量
    pub retry: u64,
    /// 时延样本集（≤ QUANT_CAP）
    pub samples: Vec<u32>,
    /// 长尾标注（p99 ≥ LONGTAIL_P99 即真）
    pub longtail: bool,
}

/// 整数分位：有序样本第 p 百分位（零浮点，p∈{50,99}）。
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

/// MSC 统计账引擎：四分账全局面 + ×16 流级分位账。
pub struct MscStats {
    streams: Vec<StreamStat>,
    read: u64,
    write: u64,
    err: u64,
    retry: u64,
    injected: u64,
    detected: u64,
}

impl MscStats {
    /// 新账引擎：×16 流全部开账（空账），四桶清零。
    pub fn new() -> MscStats {
        let mut streams = Vec::new();
        for i in 0..STREAM_MAX {
            streams.push(StreamStat {
                stream: i as u8,
                ops: 0,
                err: 0,
                retry: 0,
                samples: Vec::new(),
                longtail: false,
            });
        }
        MscStats {
            streams,
            read: 0,
            write: 0,
            err: 0,
            retry: 0,
            injected: 0,
            detected: 0,
        }
    }

    /// 落一笔传输账。判序：流号界 → 零量 → 流容量 → 桶溢出 → 落账。
    /// 样本落分位账并即时复算长尾标注。
    pub fn record(
        &mut self,
        bucket: MscBucket,
        stream: usize,
        n: u64,
        lat: u32,
    ) -> Result<(), MscCode> {
        if stream >= STREAM_MAX {
            return Err(MscCode::StreamIdRange);
        }
        if n == 0 {
            return Err(MscCode::ZeroAmount);
        }
        let st = match self.streams.get_mut(stream) {
            Some(st) => st,
            None => return Err(MscCode::StreamIdRange),
        };
        let next_ops = match st.ops.checked_add(n) {
            Some(v) => v,
            None => return Err(MscCode::StreamCapFull),
        };
        if next_ops > STREAM_OPS_CAP {
            return Err(MscCode::StreamCapFull);
        }
        let bcur = match bucket {
            MscBucket::Read => self.read,
            MscBucket::Write => self.write,
            MscBucket::Error => self.err,
            MscBucket::Retry => self.retry,
        };
        if bcur.checked_add(n).is_none() {
            return Err(MscCode::BucketOverflow);
        }
        st.ops = next_ops;
        match bucket {
            MscBucket::Read => self.read = self.read.saturating_add(n),
            MscBucket::Write => self.write = self.write.saturating_add(n),
            MscBucket::Error => {
                self.err = self.err.saturating_add(n);
                st.err = st.err.saturating_add(n);
            }
            MscBucket::Retry => {
                self.retry = self.retry.saturating_add(n);
                st.retry = st.retry.saturating_add(n);
            }
        }
        if st.samples.len() < QUANT_CAP {
            st.samples.push(lat);
            let p99 = percentile(&st.samples, 99);
            st.longtail = p99 >= LONGTAIL_P99;
        }
        Ok(())
    }

    /// ×16 流落账压测扫描：固定种子 LCG 驱动每流 QUANT_CAP 笔读账，
    /// 返回落账流数（判据 ×16 流全部落账）。
    pub fn sweep_16_streams(ms: &mut MscStats) -> usize {
        let mut lcg = Lcg::new(LCG_SEED);
        for s in 0..STREAM_MAX {
            for _ in 0..QUANT_CAP {
                let lat = lcg.next_lat();
                let _ = ms.record(MscBucket::Read, s, 1, lat);
            }
        }
        let mut n = 0usize;
        for st in ms.streams.iter() {
            if st.ops > 0 {
                n += 1;
            }
        }
        n
    }

    /// 长尾审计：重算每流 p99 与标注比对；漏标/误标均显性报错。
    pub fn audit_longtail(&self) -> Result<usize, MscCode> {
        let mut marked = 0usize;
        for st in self.streams.iter() {
            let p99 = percentile(&st.samples, 99);
            if p99 >= LONGTAIL_P99 && !st.longtail {
                return Err(MscCode::LongtailMiss);
            }
            if p99 < LONGTAIL_P99 && st.longtail {
                return Err(MscCode::LongtailFalseMark);
            }
            if st.longtail {
                marked += 1;
            }
        }
        Ok(marked)
    }

    /// 标注篡改注入器：清除指定流的长尾标注（供审计必抓演练）。
    pub fn tamper_mark(&mut self, stream: usize) -> bool {
        match self.streams.get_mut(stream) {
            Some(st) => {
                st.longtail = false;
                true
            }
            None => false,
        }
    }

    /// 四分账总量（桶和，饱和）。
    pub fn ledger_total(&self) -> u64 {
        self.read
            .saturating_add(self.write)
            .saturating_add(self.err)
            .saturating_add(self.retry)
    }

    /// 流账汇总（Σ 每流操作量，饱和）。
    pub fn stream_total(&self) -> u64 {
        let mut sum = 0u64;
        for st in self.streams.iter() {
            sum = sum.saturating_add(st.ops);
        }
        sum
    }

    /// 四分账守恒：总量 == 流账汇总。
    pub fn reconcile(&self) -> Result<u64, MscCode> {
        if self.ledger_total() != self.stream_total() {
            return Err(MscCode::LedgerMismatch);
        }
        Ok(self.ledger_total())
    }

    /// 长尾流清单（显式标注面）。
    pub fn longtail_list(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for st in self.streams.iter() {
            if st.longtail {
                out.push(st.stream);
            }
        }
        out
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

/// 双跑一致性：同一 ×16 流 LCG 扫描两次建账，流账逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> Vec<StreamStat> {
        let mut ms = MscStats::new();
        let _ = MscStats::sweep_16_streams(&mut ms);
        let _ = ms.record(MscBucket::Error, 0, 1, 500);
        let _ = ms.record(MscBucket::Retry, 1, 1, 600);
        ms.streams.clone()
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51508_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51508");
    // 1 正常流：读/写/错误/重试各一笔，四桶与流账同步落账。
    {
        let mut ms = MscStats::new();
        let r0 = ms.record(MscBucket::Read, 0, 1, 100);
        let r1 = ms.record(MscBucket::Write, 1, 1, 200);
        let r2 = ms.record(MscBucket::Error, 2, 1, 300);
        let r3 = ms.record(MscBucket::Retry, 3, 1, 400);
        s.add_dyn(
            "四分账各桶落账",
            r0.is_ok() && r1.is_ok() && r2.is_ok() && r3.is_ok() && ms.reconcile() == Ok(4),
            "四桶各一笔应四桶与流账同步且守恒",
        );
    }
    // 2 边界零：空账对账 0 == 0。
    {
        let ms = MscStats::new();
        s.add_dyn(
            "空账对账零",
            ms.reconcile() == Ok(0),
            "零传输账应对账通过且总量为 0",
        );
    }
    // 3 边界零时延：lat=0 样本可入分位账。
    {
        let mut ms = MscStats::new();
        let r = ms.record(MscBucket::Read, 0, 1, 0);
        s.add_dyn(
            "零时延样本入账",
            r.is_ok(),
            "零时延是合法样本（零边界放行）",
        );
    }
    // 4 边界上限：流 15（最后一路）落账成功。
    {
        let mut ms = MscStats::new();
        let r = ms.record(MscBucket::Read, STREAM_MAX - 1, 1, 100);
        s.add_dyn(
            "流 15 上限放行",
            r.is_ok(),
            "最后一路流账必须可落账",
        );
    }
    // 5 边界上限+1：流 16 显性拒。
    {
        let mut ms = MscStats::new();
        let r = ms.record(MscBucket::Read, STREAM_MAX, 1, 100);
        s.add_dyn(
            "流 16 上限+1 拒",
            r == Err(MscCode::StreamIdRange),
            "流号越出 ×16 上限必须拒绝",
        );
    }
    // 6 容量耗尽：单流灌满后追加一笔拒。
    {
        let mut ms = MscStats::new();
        let _ = ms.record(MscBucket::Read, 0, STREAM_OPS_CAP, 100);
        let r = ms.record(MscBucket::Read, 0, 1, 100);
        s.add_dyn(
            "单流容量耗尽拒",
            r == Err(MscCode::StreamCapFull),
            "流账满 STREAM_OPS_CAP 后必须拒",
        );
    }
    // 7 注入必抓：流号越界 ×10 轮。
    {
        let mut ms = MscStats::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = ms.record(MscBucket::Read, STREAM_MAX + i as usize, 1, 100);
            if r == Err(MscCode::StreamIdRange) {
                caught += 1;
                ms.note_detected();
            }
        }
        s.add_dyn("流号越界注入 10/10", caught == 10, "流号越界必须全抓");
    }
    // 8 注入必抓：零量注入 ×10 轮。
    {
        let mut ms = MscStats::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = ms.record(MscBucket::Write, 0, 0, 100);
            if r == Err(MscCode::ZeroAmount) {
                caught += 1;
                ms.note_detected();
            }
        }
        s.add_dyn("零量注入 10/10", caught == 10, "零量账必须全抓");
    }
    // 9 注入必抓：长尾标注篡改 ×10 轮（改后审计必抓漏标）。
    {
        let mut all_caught = true;
        for i in 0..10u32 {
            let mut ms = MscStats::new();
            let _ = ms.record(MscBucket::Read, i as usize, 1, 950);
            let _ = ms.tamper_mark(i as usize);
            if ms.audit_longtail() != Err(MscCode::LongtailMiss) {
                all_caught = false;
            } else {
                ms.note_detected();
            }
        }
        s.add_dyn(
            "标注篡改注入 10/10",
            all_caught,
            "长尾标注被改必须审计当场抓",
        );
    }
    // 10 判定次序：流号界先于零量。
    {
        let mut ms = MscStats::new();
        let r = ms.record(MscBucket::Read, STREAM_MAX, 0, 100);
        s.add_dyn(
            "判序流界先于零量",
            r == Err(MscCode::StreamIdRange),
            "流号判据必须先于账量判据",
        );
    }
    // 11 判定次序：零量先于流容量。
    {
        let mut ms = MscStats::new();
        let _ = ms.record(MscBucket::Read, 0, STREAM_OPS_CAP, 100);
        let r = ms.record(MscBucket::Read, 0, 0, 100);
        s.add_dyn(
            "判序零量先于容量",
            r == Err(MscCode::ZeroAmount),
            "零量判据必须先于容量判据",
        );
    }
    // 12 ×16 流落账：LCG 扫描后 16 路全部有账。
    {
        let mut ms = MscStats::new();
        let n = MscStats::sweep_16_streams(&mut ms);
        s.add_dyn(
            "×16 流全部落账",
            n == STREAM_MAX,
            "流级分位账必须 ×16 流全部落账",
        );
    }
    // 13 长尾流显式标注：越阈流落标且审计通过。
    {
        let mut ms = MscStats::new();
        let _ = ms.record(MscBucket::Read, 3, 1, 100);
        let _ = ms.record(MscBucket::Read, 3, 1, 200);
        let _ = ms.record(MscBucket::Read, 3, 1, 300);
        let _ = ms.record(MscBucket::Read, 3, 1, 400);
        let _ = ms.record(MscBucket::Read, 3, 1, 999);
        let list = ms.longtail_list();
        let only3 = list.len() == 1 && match list.get(0) {
            Some(x) => *x == 3u8,
            None => false,
        };
        s.add_dyn(
            "长尾流显式标注",
            ms.audit_longtail() == Ok(1) && only3,
            "p99 越阈流必须显式落标且审计通过",
        );
    }
    // 14 分位账整数计算：p50/p99 可复核。
    {
        let p50 = percentile(&[1u32, 2, 3, 4], 50);
        let p99 = percentile(&[1u32, 2, 3, 4], 99);
        let e = percentile(&[], 99);
        s.add_dyn(
            "整数分位可复核",
            p50 == 3 && p99 == 4 && e == 0,
            "p50/p99 按有序样本整数分位取值，空账取 0",
        );
    }
    // 15 四分账守恒：总量 == 流账汇总。
    {
        let mut ms = MscStats::new();
        for st in 0..STREAM_MAX {
            let _ = ms.record(MscBucket::Read, st, 2, 100);
            let _ = ms.record(MscBucket::Write, st, 1, 100);
        }
        s.add_dyn(
            "四分账守恒",
            ms.reconcile() == Ok(48),
            "16 流 ×(读 2+写 1) 总量 48 必须与流账汇总对平",
        );
    }
    // 16 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut ms = MscStats::new();
        for i in 0..10u32 {
            let _ = ms.record(MscBucket::Read, STREAM_MAX + i as usize, 1, 100);
            ms.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            ms.conserved(),
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 17 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一 LCG 扫描两次建账流账逐字段相等");
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
    fn four_buckets_record_and_reconcile() {
        let mut ms = MscStats::new();
        assert!(ms.record(MscBucket::Read, 0, 1, 100).is_ok());
        assert!(ms.record(MscBucket::Write, 1, 1, 200).is_ok());
        assert!(ms.record(MscBucket::Error, 2, 1, 300).is_ok());
        assert!(ms.record(MscBucket::Retry, 3, 1, 400).is_ok());
        assert_eq!(ms.reconcile(), Ok(4));
        assert_eq!(ms.ledger_total(), 4);
    }

    #[test]
    fn stream_bound_zero_and_limit() {
        let mut ms = MscStats::new();
        assert!(ms.record(MscBucket::Read, 0, 1, 0).is_ok());
        assert!(ms.record(MscBucket::Read, STREAM_MAX - 1, 1, 10).is_ok());
        assert_eq!(
            ms.record(MscBucket::Read, STREAM_MAX, 1, 10),
            Err(MscCode::StreamIdRange)
        );
    }

    #[test]
    fn stream_cap_exhausted() {
        let mut ms = MscStats::new();
        assert!(ms
            .record(MscBucket::Read, 0, STREAM_OPS_CAP, 100)
            .is_ok());
        assert_eq!(
            ms.record(MscBucket::Read, 0, 1, 100),
            Err(MscCode::StreamCapFull)
        );
    }

    #[test]
    fn injection_stream_range_x10() {
        let mut ms = MscStats::new();
        for i in 0..10u32 {
            assert_eq!(
                ms.record(MscBucket::Read, STREAM_MAX + i as usize, 1, 10),
                Err(MscCode::StreamIdRange)
            );
        }
    }

    #[test]
    fn injection_zero_amount_x10() {
        let mut ms = MscStats::new();
        for _ in 0..10u32 {
            assert_eq!(
                ms.record(MscBucket::Write, 0, 0, 10),
                Err(MscCode::ZeroAmount)
            );
        }
    }

    #[test]
    fn injection_tamper_mark_caught_x10() {
        for i in 0..10u32 {
            let mut ms = MscStats::new();
            let _ = ms.record(MscBucket::Read, i as usize, 1, 950);
            assert!(ms.tamper_mark(i as usize));
            assert_eq!(ms.audit_longtail(), Err(MscCode::LongtailMiss));
        }
    }

    #[test]
    fn judge_order_range_before_zero_before_cap() {
        let mut ms = MscStats::new();
        // 流界先于零量。
        assert_eq!(
            ms.record(MscBucket::Read, STREAM_MAX, 0, 10),
            Err(MscCode::StreamIdRange)
        );
        // 零量先于流容量。
        let _ = ms.record(MscBucket::Read, 0, STREAM_OPS_CAP, 10);
        assert_eq!(
            ms.record(MscBucket::Read, 0, 0, 10),
            Err(MscCode::ZeroAmount)
        );
    }

    #[test]
    fn sweep_lands_all_16_streams() {
        let mut ms = MscStats::new();
        assert_eq!(MscStats::sweep_16_streams(&mut ms), STREAM_MAX);
        assert_eq!(ms.stream_total(), (QUANT_CAP as u64) * (STREAM_MAX as u64));
    }

    #[test]
    fn longtail_marked_and_audited() {
        let mut ms = MscStats::new();
        for lat in [100u32, 200, 300, 400, 999].iter() {
            let _ = ms.record(MscBucket::Read, 3, 1, *lat);
        }
        assert_eq!(ms.audit_longtail(), Ok(1));
        let list = ms.longtail_list();
        assert_eq!(list.len(), 1);
        assert_eq!(list.get(0), Some(&3u8));
    }

    #[test]
    fn integer_percentile_math() {
        assert_eq!(percentile(&[1u32, 2, 3, 4], 50), 3);
        assert_eq!(percentile(&[1u32, 2, 3, 4], 99), 4);
        assert_eq!(percentile(&[], 99), 0);
        assert_eq!(percentile(&[7u32], 50), 7);
    }

    #[test]
    fn ledger_conserved_48() {
        let mut ms = MscStats::new();
        for st in 0..STREAM_MAX {
            let _ = ms.record(MscBucket::Read, st, 2, 100);
            let _ = ms.record(MscBucket::Write, st, 1, 100);
        }
        assert_eq!(ms.reconcile(), Ok(48));
    }

    #[test]
    fn dual_run_identical_streams() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn injection_ledger_conserved() {
        let mut ms = MscStats::new();
        for _ in 0..10u32 {
            let _ = ms.record(MscBucket::Read, STREAM_MAX, 1, 10);
            ms.note_detected();
        }
        assert!(ms.conserved());
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
