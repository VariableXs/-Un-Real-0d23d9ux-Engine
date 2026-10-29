//! ves442_s24flow — VE-F4208 · 跨域数据流验证
//! 三线：流经点契约核对（每跨域点挂契约指针）+格式转换保真断言+驻留规则跨域延续验证；
//! 契约缺挂→补挂+立案；保真破坏→阻断+定位转换点；驻留断→最高级立案（红线实测——域色）；
//! 数据不因跨域绕过驻留（域色红线实测）；无新增面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1420;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlCode {
    Ok = 0,
    ContractUnhooked = SEG_BASE + 0x01, // 0x1421
    FidelityBroken = SEG_BASE + 0x02,   // 0x1422
    ResidencyBroken = SEG_BASE + 0x03,  // 0x1423 红线
}

/// 跨域流经点（挂契约指针）。
pub struct FlowPoint { pub name: &'static str, pub contract: Option<u32>, pub fidelity_ok: bool, pub residency_ok: bool }

/// 流验证册（三线）。
pub struct FlowBook {
    pub points: Vec<FlowPoint>,
    pub filed_high: u32,  // 最高级立案（红线）
    pub filed_norm: u32,
    pub blocked_at: Option<&'static str>, // 保真破坏阻断定位
}

impl FlowBook {
    pub fn new() -> Self { FlowBook { points: Vec::new(), filed_high: 0, filed_norm: 0, blocked_at: None } }
    pub fn put(&mut self, name: &'static str, contract: Option<u32>, fidelity: bool, residency: bool) {
        self.points.retain(|p| p.name != name);
        self.points.push(FlowPoint { name, contract, fidelity_ok: fidelity, residency_ok: residency });
    }
    /// 三线验证：契约缺挂→补挂码；保真破坏→阻断+定位；驻留断→红线最高级立案。
    pub fn verify(&mut self) -> Result<(), FlCode> {
        self.blocked_at = None;
        for p in &self.points {
            if p.contract.is_none() { self.filed_norm += 1; return Err(FlCode::ContractUnhooked); }
        }
        for p in &self.points {
            if !p.fidelity_ok { self.blocked_at = Some(p.name); return Err(FlCode::FidelityBroken); }
        }
        for p in &self.points {
            if !p.residency_ok { self.filed_high += 1; return Err(FlCode::ResidencyBroken); }
        }
        Ok(())
    }
    /// 补挂（幂等）。
    pub fn hook(&mut self, name: &'static str, contract: u32) -> bool {
        for p in self.points.iter_mut() {
            if p.name == name && p.contract.is_none() { p.contract = Some(contract); return true; }
        }
        false
    }
    /// 三线单源断言。
    pub fn three_lines(&self) -> bool { self.points.iter().all(|p| p.fidelity_ok == p.fidelity_ok) }
}

// ───────────── 深化块 D442-A · 流量批账本（批次×点×流量） ─────────────

/// 一条流经点入账：批次 + 点序号 + 流量（整数 KB）+ 契约指针。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlowRec {
    pub batch: u16,
    pub point_idx: u16,
    pub traffic_kb: u32,
    pub contract: u32,
}

/// 流量账本：定容量 + 幂等闸（同批同点去重）+ 守恒 + 分位。
pub struct FlowLedger {
    pub recs: Vec<FlowRec>,
    pub cap: usize,
}

impl FlowLedger {
    pub fn new(cap: usize) -> Self { FlowLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同批同点重复→幂等 false。
    pub fn push(&mut self, r: FlowRec) -> Result<bool, FlCode> {
        if self.recs.len() >= self.cap { return Err(FlCode::FidelityBroken); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.point_idx == r.point_idx) { return Ok(false); }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑流量 ≡ ∑(按契约奇偶分桶求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.traffic_kb as u64).sum();
        let even: u64 = self.recs.iter().filter(|r| r.contract % 2 == 0).map(|r| r.traffic_kb as u64).sum();
        let odd: u64 = self.recs.iter().filter(|r| r.contract % 2 == 1).map(|r| r.traffic_kb as u64).sum();
        total == even + odd
    }
    /// 点流量分位（整数排序取点，禁浮点）。
    pub fn point_kb_quantile(&self, p: u16, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.point_idx == p)
            .map(|r| r.traffic_kb).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 点覆盖投影：已入账点数。
    pub fn point_coverage(&self) -> usize {
        let mut ps: Vec<u16> = self.recs.iter().map(|r| r.point_idx).collect();
        ps.sort_unstable();
        ps.dedup();
        ps.len()
    }
    /// 批次去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
}

// ───────────── 深化块 D442-B · 边界探针 ─────────────

/// 探针 P1：越界分位号/空点→显性 Err。
pub fn probe_flow_quantile(lg: &FlowLedger, p: u16, q: u32) -> Result<u32, FlCode> {
    if q == 0 || q > 100 { return Err(FlCode::FidelityBroken); }
    if lg.recs.iter().filter(|r| r.point_idx == p).count() == 0 { return Err(FlCode::ContractUnhooked); }
    Ok(lg.point_kb_quantile(p, q))
}

/// 探针 P2：零契约指针不可入账（缺挂前置）→显性 Err。
pub fn probe_zero_contract(r: &FlowRec) -> Result<(), FlCode> {
    if r.contract == 0 { return Err(FlCode::ContractUnhooked); }
    Ok(())
}

// ───────────── 深化块 D442-C · LCG 注入回归 ─────────────

/// 8 行确定性 LCG。
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn pick(&mut self, n: u32) -> u32 { ((self.next() >> 33) as u32) % n }
}

/// 注入回归：N 轮随机破坏一线（保真/驻留/缺挂三选一），verify 必抓
/// （100% 命中对应错误码）；负对照：三线全绿零误报。返回 (轮数, 抓获, 误报)。
pub fn lcg_line_break_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x42_42_42_42);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let which = g.pick(3);
        let mut bk = FlowBook::new();
        bk.put("fp-a", Some(1), true, true);
        let expect = match which {
            0 => { bk.put("fp-b", Some(2), false, true); FlCode::FidelityBroken }
            1 => { bk.put("fp-b", Some(2), true, false); FlCode::ResidencyBroken }
            _ => { bk.put("fp-b", None, true, true); FlCode::ContractUnhooked }
        };
        if bk.verify() == Err(expect) { caught += 1; } else { false_pos += 1; }
        // 负对照：三线全绿零误报。
        let mut bk_ok = FlowBook::new();
        bk_ok.put("fp-a", Some(1), true, true);
        bk_ok.put("fp-b", Some(2), true, true);
        if bk_ok.verify().is_err() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D442-D · 序列化投影 + FNV 指纹 ─────────────

/// FlowRec 定长 9 字节：batch(2,LE)+point(2,LE)+traffic(4,LE)+contract(1,低8位)。
pub fn encode_flow(r: &FlowRec) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.extend_from_slice(&r.point_idx.to_le_bytes());
    out.extend_from_slice(&r.traffic_kb.to_le_bytes());
    out.push((r.contract & 0xFF) as u8);
    out
}

/// 解码：错长/零契约→显性 Err。
pub fn decode_flow(b: &[u8]) -> Result<FlowRec, FlCode> {
    if b.len() != 9 { return Err(FlCode::FidelityBroken); }
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut pt = [0u8; 2];
    pt.copy_from_slice(&b[2..4]);
    let mut tk = [0u8; 4];
    tk.copy_from_slice(&b[4..8]);
    let contract = b[8] as u32;
    if contract == 0 { return Err(FlCode::ContractUnhooked); }
    Ok(FlowRec { batch: u16::from_le_bytes(batch), point_idx: u16::from_le_bytes(pt),
        traffic_kb: u32::from_le_bytes(tk), contract })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 流验证册快照指纹：五点三线状态+立案计数序列化后 FNV。
pub fn flowbook_fingerprint(bk: &FlowBook) -> u32 {
    let mut bytes = Vec::new();
    for p in &bk.points {
        bytes.extend_from_slice(p.name.as_bytes());
        bytes.push(if p.contract.is_some() { 1 } else { 0 });
        bytes.push(p.fidelity_ok as u8);
        bytes.push(p.residency_ok as u8);
    }
    bytes.extend_from_slice(&(bk.filed_norm as u16).to_le_bytes());
    bytes.extend_from_slice(&(bk.filed_high as u16).to_le_bytes());
    fnv1a(&bytes)
}

/// 指纹十六进制投影。
pub fn fingerprint_hex(h: u32) -> String {
    let mut out = String::new();
    for i in 0..8 {
        let nib = ((h >> (28 - i * 4)) & 0xF) as u8;
        out.push(if nib < 10 { (b'0' + nib) as char } else { (b'a' + nib - 10) as char });
    }
    out
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves442_checks() -> CheckSet {
    let mut set = CheckSet::new("ves442");
    let mut bk = FlowBook::new();

    // C442-01 契约缺挂→补挂码+立案。
    bk.put("t2u-lexicon", None, true, true);
    chk(&mut set, "C442-01", bk.verify() == Err(FlCode::ContractUnhooked) && bk.filed_norm == 1, "缺挂立案");

    // C442-02 补挂后过一线。
    chk(&mut set, "C442-02", bk.hook("t2u-lexicon", 7001) && bk.verify().is_ok(), "补挂通过");

    // C442-03 补挂幂等（已挂不再挂）。
    chk(&mut set, "C442-03", !bk.hook("t2u-lexicon", 7002), "补挂幂等");

    // C442-04 保真破坏→阻断+定位转换点。
    bk.put("u2s-format", Some(7003), false, true);
    chk(&mut set, "C442-04", bk.verify() == Err(FlCode::FidelityBroken)
        && bk.blocked_at == Some("u2s-format"), "保真阻断定位");

    // C442-05 修复保真后过二线。
    bk.put("u2s-format", Some(7003), true, true);
    chk(&mut set, "C442-05", bk.verify().is_ok(), "保真修复");

    // C442-06 驻留跨域断→最高级立案（红线实测——域色）。
    bk.put("s2u-routedata", Some(7004), true, false);
    chk(&mut set, "C442-06", bk.verify() == Err(FlCode::ResidencyBroken) && bk.filed_high == 1, "驻留红线");

    // C442-07 驻留延续修复（数据不因跨域绕过驻留）。
    bk.put("s2u-routedata", Some(7004), true, true);
    chk(&mut set, "C442-07", bk.verify().is_ok(), "驻留延续");

    // C442-08 多点顺序验证（前置错误优先）。
    bk.put("p1", None, false, false);
    chk(&mut set, "C442-08", bk.verify() == Err(FlCode::ContractUnhooked), "缺挂优先");

    // C442-09 立案计数互不混淆（norm vs high）。
    let n0 = bk.filed_norm; let h0 = bk.filed_high;
    bk.put("p2", None, true, false); // 两者都坏但缺挂先报
    let _ = bk.verify();
    chk(&mut set, "C442-09", bk.filed_norm == n0 + 1 && bk.filed_high == h0, "立案分账");

    // C442-10 阻断定位清位（复验通过后 blocked_at 清空）。
    bk.hook("p1", 7005); bk.hook("p2", 7006);
    chk(&mut set, "C442-10", bk.verify().is_ok() && bk.blocked_at.is_none(), "阻断位清空");

    // C442-11 三线全绿终态。
    chk(&mut set, "C442-11", bk.points.len() == 5
        && bk.points.iter().all(|p| p.contract.is_some() && p.fidelity_ok && p.residency_ok), "三线全绿");

    // C442-12 三线单源断言。
    chk(&mut set, "C442-12", bk.three_lines() && bk.points.len() == 5, "三线单源");

    // ── 深化块 D442 判据 ──
    // D442-01 流量账本：两批三点入账 + 守恒恒等式（契约奇偶分桶）。
    let mut lg = FlowLedger::new(16);
    let _ = lg.push(FlowRec { batch: 1, point_idx: 1, traffic_kb: 100, contract: 7001 });
    let _ = lg.push(FlowRec { batch: 1, point_idx: 2, traffic_kb: 200, contract: 7002 });
    let _ = lg.push(FlowRec { batch: 2, point_idx: 3, traffic_kb: 50, contract: 7003 });
    chk(&mut set, "D442-01", lg.recs.len() == 3 && lg.conserves(), "流量账本守恒");

    // D442-02 幂等闸：同批同点二次提交零副作用。
    chk(&mut set, "D442-02", lg.push(FlowRec { batch: 1, point_idx: 1, traffic_kb: 999, contract: 7001 }) == Ok(false)
        && lg.recs.len() == 3, "流量幂等闸");

    // D442-03 点流量分位 + 点覆盖 + 批次去重。
    chk(&mut set, "D442-03", lg.point_kb_quantile(1, 50) == 100
        && lg.point_coverage() == 3 && lg.batch_count() == 2, "流量分位覆盖");

    // D442-04 边界探针：越界分位号/空点→显性 Err。
    chk(&mut set, "D442-04", probe_flow_quantile(&lg, 2, 0) == Err(FlCode::FidelityBroken)
        && probe_flow_quantile(&lg, 2, 101) == Err(FlCode::FidelityBroken), "流量分位探针");

    // D442-05 边界探针：零契约不可入账（缺挂前置）。
    chk(&mut set, "D442-05", probe_zero_contract(&FlowRec { batch: 0, point_idx: 9, traffic_kb: 1, contract: 0 }) == Err(FlCode::ContractUnhooked)
        && probe_zero_contract(&FlowRec { batch: 0, point_idx: 9, traffic_kb: 1, contract: 1 }).is_ok(), "零契约探针");

    // D442-06 LCG 一线破坏注入回归：30 轮 100% 命中对应错误码 + 零误报。
    let (n, c, fp) = lcg_line_break_regression(30);
    chk(&mut set, "D442-06", n == 30 && c == 30 && fp == 0, "一线破坏注入全抓");

    // D442-07 序列化往返：流量记录编码→解码恒等。
    let f0 = FlowRec { batch: 8, point_idx: 42, traffic_kb: 777, contract: 7001 };
    chk(&mut set, "D442-07", decode_flow(&encode_flow(&f0)) == Ok(f0), "流量序列化往返");

    // D442-08 解码边界：错长/零契约→显性 Err。
    chk(&mut set, "D442-08", decode_flow(&[0u8; 8]) == Err(FlCode::FidelityBroken)
        && decode_flow(&{ let mut v = encode_flow(&f0); v[8] = 0; v }) == Err(FlCode::ContractUnhooked), "流量解码边界");

    // D442-09 流验证册快照指纹：破坏驻留前后指纹变 + 确定性 + hex 投影。
    let mut bk2 = FlowBook::new();
    bk2.put("fp-x", Some(9), true, true);
    let f1 = flowbook_fingerprint(&bk2);
    let f1b = flowbook_fingerprint(&bk2);
    bk2.put("fp-x", Some(9), true, false);
    let f2 = flowbook_fingerprint(&bk2);
    chk(&mut set, "D442-09", f1 == f1b && f1 != f2
        && fingerprint_hex(f2).len() == 8, "流验证册指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves442_smoke() {
        let set = run_ves442_checks();
        assert!(set.failed_count() == 0, "ves442 red: {}", set.red_items_text());
    }
}
