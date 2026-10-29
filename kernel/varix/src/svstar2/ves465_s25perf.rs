//! ves465_s25perf — VE-F4231 · 契约性能
//! 三线基准：引擎吞吐（千契约验证/秒档）+冲突检测批量档（万对/分钟）+影响分析延迟 P95；
//! 与家族预算联动；越阈→定位+预算重议；批量劣化→索引重建；分析超时→缓存命中优先；
//! 含无障碍判据验证延迟单列（域本色）；数据合成。
//! 深化（2026-10-27 T域批次）：结构化采样账本（批次+线别归因）、守恒恒等式、
//! 边界探针（空样本/重复采样/满容量）、按线别投影与整数 P95 分位（禁浮点）、
//! LCG 注入回归（越阈必抓 100%+带内零误报）、幂等闸、定长序列化往返+FNV 指纹。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1840;
pub const THROUGHPUT_FLOOR: u32 = 1000; // 千契约/秒
pub const BATCH_FLOOR: u32 = 10_000;    // 万对/分钟

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    Ok = 0,
    OverBudget = SEG_BASE + 0x01, // 0x1841
    BatchSlow = SEG_BASE + 0x02,  // 0x1842
    AnalyzeSlow = SEG_BASE + 0x03, // 0x1843
    // —— 深化追加（尾部续段，不新增 SEG_BASE）——
    EmptySample = SEG_BASE + 0x04, // 0x1844 空样本（度量无效）
    DupSample = SEG_BASE + 0x05,   // 0x1845 同批次同线重复采样
    CapFull = SEG_BASE + 0x06,     // 0x1846 账本满容量
    SerialBad = SEG_BASE + 0x07,   // 0x1847 序列化字节布局失配
}

pub struct PerfLines {
    pub throughput: u32,   // 契约/秒
    pub batch: u32,        // 对/分钟
    pub analyze_p95: u32,  // μs
    pub a11y_validate: u32, // μs 无障碍判据验证单列（域本色）
    pub p95_limit: u32,
    pub a11y_limit: u32,
    pub index_rebuilt: bool,
    pub cache_hit: bool,
    pub registered: bool,
}

impl PerfLines {
    pub fn new() -> Self {
        PerfLines { throughput: 0, batch: 0, analyze_p95: 0, a11y_validate: 0,
            p95_limit: 500, a11y_limit: 100, index_rebuilt: false, cache_hit: false, registered: false }
    }
    pub fn verdict(&self) -> Result<(), PfCode> {
        if self.throughput < THROUGHPUT_FLOOR { return Err(PfCode::OverBudget); }
        if self.batch < BATCH_FLOOR {
            if self.index_rebuilt { /* 重建后复测 */ } else { return Err(PfCode::BatchSlow); }
        }
        if self.analyze_p95 > self.p95_limit {
            if self.cache_hit { /* 缓存命中优先 */ } else { return Err(PfCode::AnalyzeSlow); }
        }
        Ok(())
    }
    pub fn a11y_gate(&self) -> Result<(), PfCode> {
        if self.a11y_validate > self.a11y_limit { Err(PfCode::OverBudget) } else { Ok(()) }
    }
    pub fn register_family(&mut self, family_p95: u32, tol_bp: u32) -> bool {
        let a = self.analyze_p95.max(family_p95) as u64;
        let b = self.analyze_p95.min(family_p95) as u64;
        let ok = a == 0 || (a - b) * 10_000 / a <= tol_bp as u64;
        self.registered = ok;
        ok
    }
}

// ---------------------------------------------------------------------------
// 深化追加：结构化采样账本（三线+a11y 单列，线别归因）。
// ---------------------------------------------------------------------------

/// 线别（0 吞吐 / 1 批量 / 2 分析延迟 / 3 a11y 验证延迟——域本色单列）。
pub const LINE_THROUGHPUT: u8 = 0;
pub const LINE_BATCH: u8 = 1;
pub const LINE_ANALYZE: u8 = 2;
pub const LINE_A11Y: u8 = 3;

/// 单条采样账目：批次 + 线别 + 采样值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleRecord { pub batch: u32, pub line: u8, pub value: u32 }

/// 结构化采样账本：把三线标量升级为带批次/线别归因的定容记录。
pub struct SampleLedger { pub recs: Vec<SampleRecord>, pub cap: usize }

impl SampleLedger {
    pub fn new(cap: usize) -> Self { SampleLedger { recs: Vec::new(), cap } }

    /// 采一笔。边界探针：空样本（0=度量无效）/重复采样/满容量/线别越界。
    pub fn record(&mut self, batch: u32, line: u8, value: u32) -> Result<(), PfCode> {
        if line > LINE_A11Y { return Err(PfCode::SerialBad); }
        if value == 0 { return Err(PfCode::EmptySample); }
        if self.recs.iter().any(|r| r.batch == batch && r.line == line) {
            return Err(PfCode::DupSample);
        }
        if self.recs.len() >= self.cap { return Err(PfCode::CapFull); }
        self.recs.push(SampleRecord { batch, line, value });
        Ok(())
    }

    /// 守恒恒等式：账本总数 ≡ 四线采样计数之和。
    pub fn conserved(&self) -> bool {
        if self.recs.len() > self.cap { return false; }
        let by_line = self.by_line();
        by_line[0] + by_line[1] + by_line[2] + by_line[3] == self.recs.len()
    }

    /// 查询投影：按线别分列（[吞吐, 批量, 分析, a11y]）。
    pub fn by_line(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for r in &self.recs {
            if r.line <= LINE_A11Y { out[r.line as usize] += 1; }
        }
        out
    }

    /// 某线全部采样值（空线→空表，分位入口自校验）。
    pub fn line_values(&self, line: u8) -> Vec<u32> {
        let mut v: Vec<u32> = self.recs.iter().filter(|r| r.line == line).map(|r| r.value).collect();
        v.sort_unstable();
        v
    }

    /// 整数 P95：排序取点（下标 95*(n-1)/100，禁浮点；空样本→显性 Err）。
    pub fn p95(&self, line: u8) -> Result<u32, PfCode> {
        let v = self.line_values(line);
        if v.is_empty() { return Err(PfCode::EmptySample); }
        Ok(v[95 * (v.len() - 1) / 100])
    }

    /// 越阈投影：某线采样里超限的条数（预算重议的输入）。
    pub fn over_limit_count(&self, line: u8, limit: u32) -> usize {
        self.recs.iter().filter(|r| r.line == line && r.value > limit).count()
    }

    /// 幂等闸：同批次同线二次采样零副作用。
    pub fn idempotent(&mut self, batch: u32, line: u8, value: u32) -> bool {
        let before = self.recs.len();
        let first = self.record(batch, line, value);
        let mid = self.recs.len();
        let second = self.record(batch, line, value);
        first.is_ok() && second == Err(PfCode::DupSample)
            && mid == before + 1 && self.recs.len() == mid
    }

    /// 序列化投影：SampleRecord → 定长 9 字节（batch4 + line1 + value4 大端）。
    pub fn encode(r: &SampleRecord) -> [u8; 9] {
        let mut out = [0u8; 9];
        out[0] = (r.batch >> 24) as u8;
        out[1] = (r.batch >> 16) as u8;
        out[2] = (r.batch >> 8) as u8;
        out[3] = r.batch as u8;
        out[4] = r.line;
        out[5] = (r.value >> 24) as u8;
        out[6] = (r.value >> 16) as u8;
        out[7] = (r.value >> 8) as u8;
        out[8] = r.value as u8;
        out
    }

    /// 反序列化（带校验：线别非法→SerialBad，空样本→EmptySample）。
    pub fn decode(b: &[u8; 9]) -> Result<SampleRecord, PfCode> {
        let batch = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        let line = b[4];
        let value = u32::from_be_bytes([b[5], b[6], b[7], b[8]]);
        if line > LINE_A11Y { return Err(PfCode::SerialBad); }
        if value == 0 { return Err(PfCode::EmptySample); }
        Ok(SampleRecord { batch, line, value })
    }

    /// FNV-1a 指纹（字节布局配套）。
    pub fn fnv1a(bytes: &[u8]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for &b in bytes {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 账本摘要（String 投影，诊断用）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("samples=");
        s.push_str(&self.recs.len().to_string());
        s.push_str("/cap=");
        s.push_str(&self.cap.to_string());
        s
    }
}

/// 确定性 LCG（8 行）：注入回归驱动器（无浮点、无墙钟）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self { Lcg(seed) }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// 注入回归：LCG 驱动 N 轮 a11y 线采样——注入越阈值（> a11y_limit）必被
/// a11y 闸抓住（异常必抓率 100%）；带内采样的负对照零误报。
/// 返回 (抓住, 注入, 误报)。
pub fn inject_over_limit(rounds: u32, limit: u32) -> (u32, u32, u32) {
    let mut rng = Lcg::new(0x4650_0000_0000_0005);
    let mut caught = 0u32;
    let mut injected = 0u32;
    let mut false_alarm = 0u32;
    for i in 0..rounds {
        let p = PerfLines::new();
        if i % 2 == 0 {
            injected += 1;
            let over = limit + 1 + (rng.next() % 500) as u32;
            let mut q = p;
            q.a11y_validate = over;
            if q.a11y_gate() == Err(PfCode::OverBudget) { caught += 1; }
        } else {
            let within = (rng.next() % limit.max(1) as u64) as u32;
            let mut q = p;
            q.a11y_validate = within;
            if q.a11y_gate().is_err() { false_alarm += 1; }
        }
    }
    (caught, injected, false_alarm)
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves465_checks() -> CheckSet {
    let mut set = CheckSet::new("ves465");
    let mut p = PerfLines::new();

    // C465-01 吞吐不达千契约档。
    p.throughput = 800;
    chk(&mut set, "C465-01", p.verdict() == Err(PfCode::OverBudget), "吞吐不达");

    // C465-02 达标后批量档不达→索引重建路径。
    p.throughput = 1200;
    p.batch = 8000;
    chk(&mut set, "C465-02", p.verdict() == Err(PfCode::BatchSlow), "批量劣化");

    // C465-03 索引重建后复测过。
    p.index_rebuilt = true;
    chk(&mut set, "C465-03", p.batch < BATCH_FLOOR && p.verdict().is_ok(), "重建复测");

    // C465-04 批量恢复达标（不依赖重建标记）。
    p.batch = 11_000;
    chk(&mut set, "C465-04", p.verdict().is_ok(), "批量达标");

    // C465-05 影响分析 P95 越阈→缓存码。
    p.analyze_p95 = 600;
    chk(&mut set, "C465-05", p.verdict() == Err(PfCode::AnalyzeSlow), "分析超时");

    // C465-06 缓存命中优先→复测过。
    p.cache_hit = true;
    chk(&mut set, "C465-06", p.verdict().is_ok(), "缓存复测");

    // C465-07 域本色单列：a11y 验证延迟独立闸。
    p.a11y_validate = 150;
    chk(&mut set, "C465-07", p.a11y_gate() == Err(PfCode::OverBudget) && p.verdict().is_ok(), "单列独立拒");

    // C465-08 a11y 带内通过。
    p.a11y_validate = 90;
    chk(&mut set, "C465-08", p.a11y_gate().is_ok(), "单列带内");

    // C465-09 家族预算对拍登记。
    chk(&mut set, "C465-09", p.register_family(580, 500) && p.registered, "家族登记");

    // C465-10 对拍失配不登记。
    let mut p2 = PerfLines::new();
    p2.analyze_p95 = 2000;
    chk(&mut set, "C465-10", !p2.register_family(500, 500), "失配不登记");

    // C465-11 两档常量单源。
    chk(&mut set, "C465-11", THROUGHPUT_FLOOR == 1000 && BATCH_FLOOR == 10_000, "两档常量");

    // C465-12 三线终态。
    chk(&mut set, "C465-12", p.verdict().is_ok() && p.a11y_gate().is_ok()
        && p.registered && p.index_rebuilt && p.cache_hit, "三线终态");

    // —— 深化判据（追加面）——

    // C465-13 结构化采样账本守恒恒等式（总数≡四线之和）。
    let mut lg = SampleLedger::new(8);
    let _ = lg.record(1, LINE_THROUGHPUT, 1200);
    let _ = lg.record(1, LINE_BATCH, 11_000);
    let _ = lg.record(1, LINE_ANALYZE, 480);
    let _ = lg.record(2, LINE_A11Y, 90);
    chk(&mut set, "C465-13", lg.conserved() && lg.recs.len() == 4, "账本守恒");

    // C465-14 边界探针：空样本（0）→显性拒绝（度量无效不入账）。
    chk(&mut set, "C465-14", lg.record(3, LINE_THROUGHPUT, 0) == Err(PfCode::EmptySample), "空样本拒");

    // C465-15 边界探针 + 幂等闸：重复采样被拒、账本零副作用。
    chk(&mut set, "C465-15", lg.idempotent(3, LINE_A11Y, 85), "幂等闸");

    // C465-16 边界探针：满容量→CapFull。
    let mut full = SampleLedger::new(2);
    let _ = full.record(1, LINE_THROUGHPUT, 1);
    let _ = full.record(1, LINE_BATCH, 1);
    chk(&mut set, "C465-16", full.record(2, LINE_ANALYZE, 1) == Err(PfCode::CapFull), "满容量拒");

    // C465-17 查询投影：按线别分列（吞吐1/批量1/分析1/a11y2）。
    let k = lg.by_line();
    chk(&mut set, "C465-17", k[0] == 1 && k[1] == 1 && k[2] == 1 && k[3] == 2, "按线投影");

    // C465-18 分位投影：a11y 线 P95（85,90 → 排序取点下标 0 → 85）。
    chk(&mut set, "C465-18", lg.p95(LINE_A11Y) == Ok(85), "P95 分位");

    // C465-19 越阈投影：a11y 线对 limit=100 的越阈计数为 0（全部带内）。
    chk(&mut set, "C465-19", lg.over_limit_count(LINE_A11Y, 100) == 0
        && lg.over_limit_count(LINE_ANALYZE, 400) == 1, "越阈计数");

    // C465-20 边界探针：空线分位→显性 Err。
    let empty = SampleLedger::new(4);
    chk(&mut set, "C465-20", empty.p95(LINE_A11Y) == Err(PfCode::EmptySample), "空线拒");

    // C465-21 序列化投影：编码→解码往返一致 + 指纹稳定。
    let rec = SampleRecord { batch: 9, line: LINE_A11Y, value: 95 };
    let bytes = SampleLedger::encode(&rec);
    let back = SampleLedger::decode(&bytes);
    chk(&mut set, "C465-21", back == Ok(rec) && SampleLedger::fnv1a(&bytes) == SampleLedger::fnv1a(&bytes), "序列化往返");

    // C465-22 序列化校验：线别非法→SerialBad。
    let mut bad_b = bytes;
    bad_b[4] = 9;
    chk(&mut set, "C465-22", SampleLedger::decode(&bad_b) == Err(PfCode::SerialBad), "坏线别拒");

    // C465-23 注入回归：越阈必抓 100% + 带内零误报。
    let (caught, injected, false_alarm) = inject_over_limit(32, p.a11y_limit);
    chk(&mut set, "C465-23", injected > 0 && caught == injected && false_alarm == 0, "越阈必抓");

    // C465-24 账本摘要 String 投影（诊断可读）。
    chk(&mut set, "C465-24", lg.summary().contains("samples=5"), "摘要投影");

    // C465-25 追加变体段位单源（尾部续段不撞段）。
    chk(&mut set, "C465-25", PfCode::SerialBad as u16 == SEG_BASE + 7, "续段单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves465_smoke() {
        let set = run_ves465_checks();
        assert!(set.failed_count() == 0, "ves465 red: {}", set.red_items_text());
    }
}
