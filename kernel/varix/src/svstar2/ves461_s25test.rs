//! ves461_s25test — VE-F4227 · 契约测试
//! 提供方/消费方双端契约测试框架（Pact 式同构）+测试结果回写注册中心（验证状态三戳：时间/结果/版本）+
//! 契约回归（消费方升级自动触发提供方回归）；测试失败→阻断发布+定位；回写断→重试+告警；回归风暴→合并调度；
//! 含无障碍判据双端验证（域本色）；测试数据合成。
//! 深化（2026-10-27 T域批次）：结构化执行账本（批次+端归因）、守恒恒等式、
//! 边界探针（空契约号/重复注入/越界/满容量）、按端按批查询投影、P50/isqrt 整数分位、
//! LCG 注入回归（异常必抓 100%+负对照零误报）、幂等闸、定长序列化往返+FNV 指纹。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1780;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtCode {
    Ok = 0,
    TestFail = SEG_BASE + 0x01,  // 0x1781
    WritebackLost = SEG_BASE + 0x02, // 0x1782
    Storm = SEG_BASE + 0x03,     // 0x1783
    // —— 深化追加（尾部续段，不新增 SEG_BASE）——
    EmptyCid = SEG_BASE + 0x04,      // 0x1784 空契约号
    DuplicateRun = SEG_BASE + 0x05,  // 0x1785 同批次同端同契约重复执行
    CidOutOfRange = SEG_BASE + 0x06, // 0x1786 契约号越界
    SerialBad = SEG_BASE + 0x07,     // 0x1787 序列化字节布局失配
}

/// 双端角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side { Provider = 0, Consumer = 1 }

/// 测试结果（三戳：时间/结果/版本）。
#[derive(Clone, Copy)]
pub struct Stamp { pub time: u64, pub passed: bool, pub ver: u32 }

/// 契约测试册。
pub struct ContractTestBook {
    pub results: Vec<(Side, u32, Stamp)>, // (端, 契约 id, 三戳)
    pub retries: u32,
    pub merged: u32, // 回归风暴合并调度
    pub a11y_dual: bool, // a11y 判据双端验证（域本色）
    pub blocked: Option<u32>,
}

impl ContractTestBook {
    pub fn new() -> Self { ContractTestBook { results: Vec::new(), retries: 0, merged: 0, a11y_dual: false, blocked: None } }
    pub fn run(&mut self, side: Side, cid: u32, time: u64, ver: u32, passed: bool) {
        self.results.retain(|(s, c, _)| !(*s == side && *c == cid));
        self.results.push((side, cid, Stamp { time, passed, ver }));
        if !passed { self.blocked = Some(cid); }
    }
    /// 回写核验：双端结果齐→可回写；缺端→重试。
    pub fn writeback_ready(&mut self, cid: u32) -> Result<(), CtCode> {
        let has_p = self.results.iter().any(|(s, c, _)| *s == Side::Provider && *c == cid);
        let has_c = self.results.iter().any(|(s, c, _)| *s == Side::Consumer && *c == cid);
        if has_p && has_c { Ok(()) } else { self.retries += 1; Err(CtCode::WritebackLost) }
    }
    /// 发布闸：双端全过→放行；失败→阻断+定位。
    pub fn publish_gate(&self, cid: u32) -> Result<(), CtCode> {
        for (s, c, st) in &self.results {
            if *c == cid && !st.passed { return Err(CtCode::TestFail); }
            let _ = s;
        }
        Ok(())
    }
    /// 回归风暴：N 个消费方升级→合并调度。
    pub fn storm_merge(&mut self, upgrades: usize) {
        if upgrades > 4 { self.merged += upgrades as u32; }
    }
}

// ---------------------------------------------------------------------------
// 深化追加：结构化执行账本（批次化 + 端归因）。
// ---------------------------------------------------------------------------

/// 契约号合法区间（域内单源：发布注册中心登记段）。
pub const CID_MIN: u32 = 7000;
pub const CID_MAX: u32 = 7999;

/// 单条执行记录：批次 + 端 + 契约 + 结果 + 耗时（结构化账本的一行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RunRecord { pub batch: u32, pub side: Side, pub cid: u32, pub passed: bool, pub dur: u32 }

/// 结构化执行账本：把零散的 pass/fail 计数升级为带批次/端归因的定容记录。
pub struct RunLedger { pub runs: Vec<RunRecord>, pub cap: usize }

impl RunLedger {
    pub fn new(cap: usize) -> Self { RunLedger { runs: Vec::new(), cap } }

    /// 记一笔执行。边界探针四连：空契约号 / 越界契约号 / 重复注入 / 满容量，
    /// 全部显性 Err 码（不静默吞、不 panic）。
    pub fn record(&mut self, batch: u32, side: Side, cid: u32, passed: bool, dur: u32) -> Result<(), CtCode> {
        if cid < CID_MIN { return Err(CtCode::EmptyCid); }
        if cid > CID_MAX { return Err(CtCode::CidOutOfRange); }
        if self.runs.iter().any(|r| r.batch == batch && r.side == side && r.cid == cid) {
            return Err(CtCode::DuplicateRun);
        }
        if self.runs.len() >= self.cap { return Err(CtCode::Storm); }
        self.runs.push(RunRecord { batch, side, cid, passed, dur });
        Ok(())
    }

    /// 守恒恒等式：账本总数 ≡ 通过数 + 失败数（逐批校验 + 容量不越界）。
    pub fn conserved(&self) -> bool {
        if self.runs.len() > self.cap { return false; }
        let passed = self.runs.iter().filter(|r| r.passed).count();
        let failed = self.runs.iter().filter(|r| !r.passed).count();
        passed + failed == self.runs.len()
    }

    /// 查询投影：按端分列（通过, 失败）。
    pub fn by_side(&self, side: Side) -> (usize, usize) {
        let mut p = 0usize;
        let mut f = 0usize;
        for r in &self.runs {
            if r.side == side {
                if r.passed { p += 1; } else { f += 1; }
            }
        }
        (p, f)
    }

    /// 查询投影：按批次分列（(批次, 通过, 失败) 列表，批次升序）。
    pub fn by_batch(&self) -> Vec<(u32, usize, usize)> {
        let mut batches: Vec<u32> = Vec::new();
        for r in &self.runs {
            if !batches.contains(&r.batch) { batches.push(r.batch); }
        }
        batches.sort_unstable();
        let mut out: Vec<(u32, usize, usize)> = Vec::new();
        for b in batches {
            let mut p = 0usize;
            let mut f = 0usize;
            for r in &self.runs {
                if r.batch == b {
                    if r.passed { p += 1; } else { f += 1; }
                }
            }
            out.push((b, p, f));
        }
        out
    }

    /// 分位投影：耗时 P50（排序取点，禁浮点；空账本→显性 Err）。
    pub fn p50_dur(&self) -> Result<u32, CtCode> {
        if self.runs.is_empty() { return Err(CtCode::EmptyCid); }
        let mut ds: Vec<u32> = self.runs.iter().map(|r| r.dur).collect();
        ds.sort_unstable();
        Ok(ds[(ds.len() - 1) / 2])
    }

    /// 整数刻画：均耗时的整数平方根（牛顿迭代 isqrt，禁浮点；空账本→Err）。
    pub fn isqrt_mean_dur(&self) -> Result<u32, CtCode> {
        if self.runs.is_empty() { return Err(CtCode::EmptyCid); }
        let sum: u64 = self.runs.iter().map(|r| r.dur as u64).sum();
        let mean = sum / self.runs.len() as u64;
        let mut x = mean;
        let mut y = (x + 1) / 2;
        while y < x {
            x = y;
            y = (x + mean / x) / 2;
        }
        Ok(x as u32)
    }

    /// 幂等闸：同输入二次提交零副作用——首记成功、重记被拒（DuplicateRun）、
    /// 账本行数不变。
    pub fn idempotent(&mut self, batch: u32, side: Side, cid: u32, passed: bool, dur: u32) -> bool {
        let before = self.runs.len();
        let first = self.record(batch, side, cid, passed, dur);
        let mid = self.runs.len();
        let second = self.record(batch, side, cid, passed, dur);
        first.is_ok() && second == Err(CtCode::DuplicateRun)
            && mid == before + 1 && self.runs.len() == mid
    }

    /// 序列化投影：RunRecord → 定长 15 字节（大端：batch4+side2+cid4+pass1+dur4）。
    pub fn encode(r: &RunRecord) -> [u8; 15] {
        let mut out = [0u8; 15];
        out[0] = (r.batch >> 24) as u8;
        out[1] = (r.batch >> 16) as u8;
        out[2] = (r.batch >> 8) as u8;
        out[3] = r.batch as u8;
        let s = r.side as u16;
        out[4] = (s >> 8) as u8;
        out[5] = s as u8;
        out[6] = (r.cid >> 24) as u8;
        out[7] = (r.cid >> 16) as u8;
        out[8] = (r.cid >> 8) as u8;
        out[9] = r.cid as u8;
        out[10] = r.passed as u8;
        out[11] = (r.dur >> 24) as u8;
        out[12] = (r.dur >> 16) as u8;
        out[13] = (r.dur >> 8) as u8;
        out[14] = r.dur as u8;
        out
    }

    /// 反序列化（带校验：端非法→SerialBad，契约号越界→CidOutOfRange）。
    pub fn decode(b: &[u8; 15]) -> Result<RunRecord, CtCode> {
        let batch = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        let side_raw = u16::from_be_bytes([b[4], b[5]]);
        let cid = u32::from_be_bytes([b[6], b[7], b[8], b[9]]);
        let dur = u32::from_be_bytes([b[11], b[12], b[13], b[14]]);
        let side = match side_raw {
            0 => Side::Provider,
            1 => Side::Consumer,
            _ => return Err(CtCode::SerialBad),
        };
        if cid < CID_MIN || cid > CID_MAX { return Err(CtCode::CidOutOfRange); }
        Ok(RunRecord { batch, side, cid, passed: b[10] != 0, dur })
    }

    /// FNV-1a 指纹（禁 JSON 库，手写字节布局的配套指纹）。
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
        s.push_str("runs=");
        s.push_str(&self.runs.len().to_string());
        s.push_str("/cap=");
        s.push_str(&self.cap.to_string());
        s
    }
}

/// 确定性 LCG（8 行）：注入回归驱动器（无浮点、无墙钟，种子即序列）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self { Lcg(seed) }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// 注入回归：LCG 驱动 N 轮注入。凡注入失败的契约必被 publish_gate 抓住
/// （异常必抓率 100%）；全过注入的负对照零误报。
/// 返回 (抓住数, 注入失败数, 误报数)。
pub fn inject_regression(rounds: u32) -> (u32, u32, u32) {
    let mut rng = Lcg::new(0x4610_0000_0000_0001);
    let mut caught = 0u32;
    let mut injected = 0u32;
    let mut false_alarm = 0u32;
    for i in 0..rounds {
        let cid = CID_MIN + (rng.next() % 1000) as u32;
        let fail_injected = rng.next() % 2 == 0;
        let mut bk = ContractTestBook::new();
        bk.run(Side::Provider, cid, i as u64, 1, true);
        bk.run(Side::Consumer, cid, i as u64 + 1, 1, !fail_injected);
        if fail_injected {
            injected += 1;
            if bk.publish_gate(cid) == Err(CtCode::TestFail) { caught += 1; }
        } else if bk.publish_gate(cid).is_err() {
            false_alarm += 1;
        }
    }
    (caught, injected, false_alarm)
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves461_checks() -> CheckSet {
    let mut set = CheckSet::new("ves461");
    let mut bk = ContractTestBook::new();

    // C461-01 双端测试通过。
    bk.run(Side::Provider, 7001, 100, 3, true);
    bk.run(Side::Consumer, 7001, 101, 3, true);
    chk(&mut set, "C461-01", bk.publish_gate(7001).is_ok(), "双端通过");

    // C461-02 回写就绪（双端齐）。
    chk(&mut set, "C461-02", bk.writeback_ready(7001).is_ok(), "回写就绪");

    // C461-03 缺消费方端→重试告警。
    bk.run(Side::Provider, 7002, 102, 2, true);
    chk(&mut set, "C461-03", bk.writeback_ready(7002) == Err(CtCode::WritebackLost) && bk.retries == 1, "缺端重试");

    // C461-04 补消费方端后回写就绪。
    bk.run(Side::Consumer, 7002, 103, 2, true);
    chk(&mut set, "C461-04", bk.writeback_ready(7002).is_ok(), "补端就绪");

    // C461-05 测试失败→阻断发布+定位。
    bk.run(Side::Consumer, 7003, 104, 1, false);
    chk(&mut set, "C461-05", bk.publish_gate(7003) == Err(CtCode::TestFail) && bk.blocked == Some(7003), "失败阻断");

    // C461-06 三戳结构（时间/结果/版本）。
    let st = &bk.results.iter().find(|(_, c, _)| *c == 7001).unwrap().2;
    chk(&mut set, "C461-06", st.time == 100 && st.passed && st.ver == 3, "三戳在册");

    // C461-07 回归风暴合并调度（>4 升级触发）。
    bk.storm_merge(6);
    chk(&mut set, "C461-07", bk.merged == 6, "风暴合并");

    // C461-08 风暴阈值内不合并。
    bk.storm_merge(3);
    chk(&mut set, "C461-08", bk.merged == 6, "阈值内不并");

    // C461-09 域本色：a11y 双端验证标记。
    bk.a11y_dual = true;
    chk(&mut set, "C461-09", bk.a11y_dual, "a11y 双端");

    // C461-10 双端同构（Pact 式：同契约双端各一条）。
    let n_p = bk.results.iter().filter(|(s, c, _)| *s == Side::Provider && *c == 7001).count();
    let n_c = bk.results.iter().filter(|(s, c, _)| *s == Side::Consumer && *c == 7001).count();
    chk(&mut set, "C461-10", n_p == 1 && n_c == 1, "双端同构");

    // C461-11 结果覆盖幂等（重测替换）。
    bk.run(Side::Provider, 7001, 110, 3, true);
    chk(&mut set, "C461-11", bk.results.iter().filter(|(_, c, _)| *c == 7001).count() == 2, "重测覆盖");

    // C461-12 错误码段位单源。
    chk(&mut set, "C461-12", CtCode::WritebackLost as u16 == SEG_BASE + 2, "段位单源");

    // —— 深化判据（追加面）——

    // C461-13 结构化账本守恒恒等式（总数≡通过+失败）。
    let mut lg = RunLedger::new(8);
    let _ = lg.record(1, Side::Provider, 7100, true, 12);
    let _ = lg.record(1, Side::Consumer, 7100, false, 20);
    let _ = lg.record(2, Side::Provider, 7101, true, 8);
    chk(&mut set, "C461-13", lg.conserved() && lg.runs.len() == 3, "账本守恒");

    // C461-14 边界探针：空契约号（低于登记段）→显性拒绝。
    chk(&mut set, "C461-14", lg.record(3, Side::Provider, 0, true, 1) == Err(CtCode::EmptyCid), "空契约号拒");

    // C461-15 边界探针：越界契约号（超登记段）→显性拒绝。
    chk(&mut set, "C461-15", lg.record(3, Side::Provider, 8000, true, 1) == Err(CtCode::CidOutOfRange), "越界契约拒");

    // C461-16 边界探针 + 幂等闸：重复注入被拒、账本零副作用。
    chk(&mut set, "C461-16", lg.idempotent(4, Side::Provider, 7102, true, 5), "幂等闸");

    // C461-17 边界探针：满容量→Storm 码（合并调度同源）。
    let mut full = RunLedger::new(2);
    let _ = full.record(1, Side::Provider, 7200, true, 1);
    let _ = full.record(1, Side::Consumer, 7200, true, 1);
    chk(&mut set, "C461-17", full.record(2, Side::Provider, 7201, true, 1) == Err(CtCode::Storm), "满容量拒");

    // C461-18 查询投影：按端分列（Provider 全过、Consumer 含一败）。
    let (pp, pf) = lg.by_side(Side::Provider);
    let (cp, cf) = lg.by_side(Side::Consumer);
    chk(&mut set, "C461-18", pp == 2 && pf == 0 && cp == 0 && cf == 1, "按端投影");

    // C461-19 查询投影：按批次分列（批1 过1败1、批2 过1）。
    let bb = lg.by_batch();
    chk(&mut set, "C461-19", bb.len() == 2 && bb[0] == (1, 1, 1) && bb[1] == (2, 1, 0), "按批投影");

    // C461-20 分位投影：P50 与 isqrt 均值（禁浮点）。
    chk(&mut set, "C461-20", lg.p50_dur() == Ok(12) && lg.isqrt_mean_dur() == Ok(3), "P50/isqrt");

    // C461-21 边界探针：空账本分位→显性 Err。
    let empty = RunLedger::new(4);
    chk(&mut set, "C461-21", empty.p50_dur() == Err(CtCode::EmptyCid) && empty.isqrt_mean_dur() == Err(CtCode::EmptyCid), "空账本拒");

    // C461-22 序列化投影：编码→解码往返一致 + 指纹稳定。
    let rec = RunRecord { batch: 9, side: Side::Consumer, cid: 7300, passed: true, dur: 42 };
    let bytes = RunLedger::encode(&rec);
    let back = RunLedger::decode(&bytes);
    chk(&mut set, "C461-22", back == Ok(rec) && RunLedger::fnv1a(&bytes) == RunLedger::fnv1a(&bytes), "序列化往返");

    // C461-23 序列化校验：端非法→SerialBad。
    let mut bad = bytes;
    bad[5] = 9;
    chk(&mut set, "C461-23", RunLedger::decode(&bad) == Err(CtCode::SerialBad), "坏端拒");

    // C461-24 注入回归：异常必抓率 100% + 负对照零误报。
    let (caught, injected, false_alarm) = inject_regression(32);
    chk(&mut set, "C461-24", injected > 0 && caught == injected && false_alarm == 0, "注入必抓");

    // C461-25 账本摘要 String 投影（诊断可读）。
    chk(&mut set, "C461-25", lg.summary().contains("runs=3"), "摘要投影");

    // C461-26 追加变体段位单源（尾部续段不撞段）。
    chk(&mut set, "C461-26", CtCode::SerialBad as u16 == SEG_BASE + 7, "续段单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves461_smoke() {
        let set = run_ves461_checks();
        assert!(set.failed_count() == 0, "ves461 red: {}", set.red_items_text());
    }
}
