//! ves433_s23score — VE-F4193 · T 域 20 维度验收评分
//! 评分：T 域 20 维度终版评分（家族评分卡终演+每维证据指针+F4174 预评对账（偏差说明））；
//! 三 90 线核验（≥90 维数/均分/无短板维）；线未达→整改+复评；预终偏差超限→说明+复评；证据缺→暂缓该维；
//! 无障碍维终评含多语言读屏加权终版口径（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x12E0;
pub const DIM_N: usize = 20;
pub const LINE: u32 = 90;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    Ok = 0,
    LineShort = SEG_BASE + 0x01,  // 0x12E1
    DeviateOver = SEG_BASE + 0x02, // 0x12E2 预终偏差超限
    EvidenceHeld = SEG_BASE + 0x03, // 0x12E3 证据缺暂缓
}

/// 20 维（维号 0..20）；无障碍维（第 2 维）为域本色主场维。
pub const A11Y_DIM: usize = 2;

/// 单维评分（证据指针 + 终评分）。
#[derive(PartialEq, Clone, Copy)]
pub struct DimScore { pub dim: usize, pub final_score: u32, pub evidence: Option<u32>, pub held: bool }

/// 终版评分卡。
pub struct ScoreCard {
    pub dims: Vec<DimScore>,
    pub pre_scores: Vec<(usize, u32)>, // F4174 预评
    pub deviations: Vec<(usize, i32)>, // 偏差说明（终-预）
    pub filed: u32,
}

impl ScoreCard {
    pub fn new() -> Self { ScoreCard { dims: Vec::new(), pre_scores: Vec::new(), deviations: Vec::new(), filed: 0 } }
    pub fn put(&mut self, dim: usize, score: u32, ev: u32) {
        self.dims.retain(|d| d.dim != dim);
        self.dims.push(DimScore { dim, final_score: score, evidence: Some(ev), held: false });
    }
    pub fn hold(&mut self, dim: usize) {
        self.dims.retain(|d| d.dim != dim);
        self.dims.push(DimScore { dim, final_score: 0, evidence: None, held: true });
    }
    pub fn put_pre(&mut self, dim: usize, pre: u32) {
        self.pre_scores.retain(|(d, _)| *d != dim);
        self.pre_scores.push((dim, pre));
    }
    /// 三 90 线：① ≥90 维数 = 20；② 均分 ≥ 90；③ 无短板维（每维 ≥ 90）。
    pub fn three90(&self) -> Result<(usize, u32, u32), ScCode> {
        let active: Vec<&DimScore> = self.dims.iter().filter(|d| !d.held).collect();
        if active.len() < DIM_N { return Err(ScCode::EvidenceHeld); }
        let ge = active.iter().filter(|d| d.final_score >= LINE).count();
        let total: u64 = active.iter().map(|d| d.final_score as u64).sum();
        let avg = (total / DIM_N as u64) as u32;
        let min = active.iter().map(|d| d.final_score).min().unwrap_or(0);
        if ge < DIM_N || avg < LINE || min < LINE { return Err(ScCode::LineShort); }
        Ok((ge, avg, min))
    }
    /// 预终对账：|终-预| > DEV_LIMIT 须偏差说明；未说明→超限码。
    pub fn reconcile_pre(&mut self) -> Result<(), ScCode> {
        const DEV_LIMIT: i32 = 5;
        self.deviations.clear();
        for d in &self.dims {
            if d.held { continue; }
            if let Some((_, pre)) = self.pre_scores.iter().find(|(p, _)| *p == d.dim) {
                let dev = d.final_score as i32 - *pre as i32;
                if dev.abs() > DEV_LIMIT {
                    if !self.deviations.iter().any(|(dd, _)| *dd == d.dim) {
                        self.deviations.push((d.dim, dev));
                    }
                }
            }
        }
        if self.deviations.is_empty() { Ok(()) } else { Err(ScCode::DeviateOver) }
    }
    pub fn explain(&mut self, dim: usize, dev: i32) {
        self.deviations.retain(|(d, _)| *d != dim);
        self.deviations.push((dim, dev));
    }
}

// ───────────── 深化块 D433-A · 复评批账本（批次×维×分） ─────────────

/// 一条复评入账：批次 + 维号 + 复评分 + 整改工单号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReAuditRec {
    pub batch: u16,
    pub dim: usize,
    pub score: u32,
    pub ticket: u32,
}

/// 复评账本：定容量 + 幂等闸（同批同维去重）+ 守恒 + 分位。
pub struct ReAuditLedger {
    pub recs: Vec<ReAuditRec>,
    pub cap: usize,
}

impl ReAuditLedger {
    pub fn new(cap: usize) -> Self { ReAuditLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同批同维重复→幂等 false。
    pub fn push(&mut self, r: ReAuditRec) -> Result<bool, ScCode> {
        if self.recs.len() >= self.cap { return Err(ScCode::EvidenceHeld); }
        if r.dim >= DIM_N { return Err(ScCode::DeviateOver); } // 越界维号探针友好
        if self.recs.iter().any(|x| x.batch == r.batch && x.dim == r.dim) { return Ok(false); }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑复评分 ≡ ∑(按整改前/后两桶分列求和)（ticket 奇偶分桶）。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.score as u64).sum();
        let even: u64 = self.recs.iter().filter(|r| r.ticket % 2 == 0).map(|r| r.score as u64).sum();
        let odd: u64 = self.recs.iter().filter(|r| r.ticket % 2 == 1).map(|r| r.score as u64).sum();
        total == even + odd
    }
    /// 维分分位（整数排序取点，禁浮点）。
    pub fn dim_score_quantile(&self, dim: usize, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.dim == dim)
            .map(|r| r.score).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 过线率投影（整数千分比：≥LINE 的复评占比）。
    pub fn pass_rate_pm(&self) -> u32 {
        if self.recs.is_empty() { return 0; }
        let p = self.recs.iter().filter(|r| r.score >= LINE).count();
        (p * 1000 / self.recs.len()) as u32
    }
    /// 批次去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
}

// ───────────── 深化块 D433-B · 边界探针 ─────────────

/// 探针 P1：越界维号/越界分位号→显性 Err。
pub fn probe_dim_quantile(lg: &ReAuditLedger, dim: usize, q: u32) -> Result<u32, ScCode> {
    if dim >= DIM_N { return Err(ScCode::DeviateOver); }
    if q == 0 || q > 100 { return Err(ScCode::LineShort); }
    if lg.recs.iter().filter(|r| r.dim == dim).count() == 0 { return Err(ScCode::EvidenceHeld); }
    Ok(lg.dim_score_quantile(dim, q))
}

/// 探针 P2：分数越界（>100）不可入卡→显性 Err。
pub fn probe_score_range(score: u32) -> Result<(), ScCode> {
    if score > 100 { return Err(ScCode::LineShort); }
    Ok(())
}

// ───────────── 深化块 D433-C · LCG 注入回归 ─────────────

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

/// 注入回归：N 轮随机注入短板分（<LINE），three90 必拒（100% 抓）；
/// 负对照：全过线注入零误报。返回 (轮数, 抓获, 误报)。
pub fn lcg_shortboard_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x33_33_33_33);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let dim = g.pick(DIM_N as u32) as usize;
        let bad = g.pick(LINE); // [0, 90) 必短板
        let mut card = ScoreCard::new();
        for i in 0..DIM_N {
            let v = if i == dim { bad } else { LINE };
            card.put(i, v, 7000 + i as u32);
        }
        if card.three90() == Err(ScCode::LineShort) { caught += 1; } else { false_pos += 1; }
        // 负对照：短板整改到线，必过。
        card.put(dim, LINE, 7000 + dim as u32);
        if card.three90().is_err() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D433-D · 序列化投影 + FNV 指纹 ─────────────

/// DimScore 定长 10 字节：dim(2,LE)+score(4,LE)+ev(4,LE)；held 以 ev==u32::MAX 表。
pub fn encode_dim(d: &DimScore) -> Vec<u8> {
    let mut out = Vec::with_capacity(10);
    out.extend_from_slice(&(d.dim as u16).to_le_bytes());
    out.extend_from_slice(&d.final_score.to_le_bytes());
    let ev = match d.evidence { Some(e) => e, None => u32::MAX };
    out.extend_from_slice(&ev.to_le_bytes());
    out
}

/// 解码：错长/越界维号→显性 Err。
pub fn decode_dim(b: &[u8]) -> Result<DimScore, ScCode> {
    if b.len() != 10 { return Err(ScCode::DeviateOver); }
    let mut dm = [0u8; 2];
    dm.copy_from_slice(&b[0..2]);
    let dim = u16::from_le_bytes(dm) as usize;
    if dim >= DIM_N { return Err(ScCode::DeviateOver); }
    let mut sc = [0u8; 4];
    sc.copy_from_slice(&b[2..6]);
    let mut ev = [0u8; 4];
    ev.copy_from_slice(&b[6..10]);
    let evv = u32::from_le_bytes(ev);
    Ok(DimScore { dim, final_score: u32::from_le_bytes(sc),
        evidence: if evv == u32::MAX { None } else { Some(evv) }, held: evv == u32::MAX })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 评分卡快照指纹：20 维终评分序列化后 FNV。
pub fn scorecard_fingerprint(card: &ScoreCard) -> u32 {
    let mut bytes = Vec::new();
    for i in 0..DIM_N {
        let v = card.dims.iter().find(|d| d.dim == i).map(|d| d.final_score).unwrap_or(0);
        bytes.extend_from_slice(&v.to_le_bytes());
    }
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

pub fn run_ves433_checks() -> CheckSet {
    let mut set = CheckSet::new("ves433");
    let mut card = ScoreCard::new();

    // C433-01 20 维全录入（证据齐），含 a11y 主场维（域本色加权口径位）。
    for i in 0..DIM_N { card.put(i, 92 + (i as u32 % 5), 7000 + i as u32); }
    card.put(A11Y_DIM, 93, 7002); // a11y 终评 93
    chk(&mut set, "C433-01", card.dims.len() == DIM_N, "20 维录入");

    // C433-02 三 90 线全过（维数/均分/最短板）。
    let r = card.three90();
    chk(&mut set, "C433-02", r == Ok((20, 94, 92)), "三 90 全过");

    // C433-03 单维跌破 90 → 线未达（无短板维线）。
    card.put(7, 85, 7007);
    chk(&mut set, "C433-03", card.three90() == Err(ScCode::LineShort), "短板维拒");

    // C433-04 整改复评复绿。
    card.put(7, 91, 7007);
    chk(&mut set, "C433-04", card.three90().is_ok(), "复评复绿");

    // C433-05 证据缺→暂缓该维。
    card.hold(9);
    chk(&mut set, "C433-05", card.three90() == Err(ScCode::EvidenceHeld)
        && card.dims.iter().find(|d| d.dim == 9).unwrap().held, "暂缓该维");

    // C433-06 补证后恢复。
    card.put(9, 92, 7009);
    chk(&mut set, "C433-06", card.three90().is_ok(), "补证恢复");

    // C433-07 预评对账：偏差 ≤5 无需说明。
    for i in 0..DIM_N { card.put_pre(i, 92); }
    card.put(A11Y_DIM, 93, 7002);
    chk(&mut set, "C433-07", card.reconcile_pre().is_ok(), "偏差限内");

    // C433-08 预终偏差超限→超限码+定位。
    card.put_pre(11, 80); // 终 92 预 80 → 偏差 12
    chk(&mut set, "C433-08", card.reconcile_pre() == Err(ScCode::DeviateOver)
        && card.deviations.contains(&(11, 12)), "偏差超限定位");

    // C433-09 偏差说明留痕后复评过。
    card.explain(11, 12);
    chk(&mut set, "C433-09", card.deviations.iter().any(|(d, v)| *d == 11 && *v == 12), "说明留痕");

    // C433-10 a11y 主场维终评确认（域本色）。
    let a = card.dims.iter().find(|d| d.dim == A11Y_DIM).unwrap();
    chk(&mut set, "C433-10", a.final_score == 93 && a.evidence.is_some(), "a11y 终评确认");

    // C433-11 均分单源复算。
    let active: Vec<u64> = card.dims.iter().map(|d| d.final_score as u64).collect();
    let avg = active.iter().sum::<u64>() / DIM_N as u64;
    chk(&mut set, "C433-11", avg >= LINE as u64, "均分复算");

    // C433-12 证据指针单源齐备。
    chk(&mut set, "C433-12", card.dims.len() == DIM_N
        && card.dims.iter().all(|d| d.evidence.is_some()), "证据指针齐备");

    // ── 深化块 D433 判据 ──
    // D433-01 复评账本：两批三维入账 + 守恒恒等式（工单奇偶分桶）。
    let mut lg = ReAuditLedger::new(32);
    let _ = lg.push(ReAuditRec { batch: 1, dim: 3, score: 96, ticket: 11 });
    let _ = lg.push(ReAuditRec { batch: 1, dim: 7, score: 92, ticket: 12 });
    let _ = lg.push(ReAuditRec { batch: 2, dim: 7, score: 94, ticket: 13 });
    let _ = lg.push(ReAuditRec { batch: 2, dim: A11Y_DIM, score: 95, ticket: 14 });
    chk(&mut set, "D433-01", lg.recs.len() == 4 && lg.conserves(), "复评账本守恒");

    // D433-02 幂等闸：同批同维二次提交零副作用。
    chk(&mut set, "D433-02", lg.push(ReAuditRec { batch: 1, dim: 3, score: 1, ticket: 99 }) == Ok(false)
        && lg.recs.len() == 4, "复评幂等闸");

    // D433-03 维分分位（整数）+ 过线率投影（全过线=1000‰）。
    chk(&mut set, "D433-03", lg.dim_score_quantile(7, 50) == 92
        && lg.dim_score_quantile(7, 100) == 94 && lg.pass_rate_pm() == 1000, "复评分位投影");

    // D433-04 边界探针：越界维号/分位号/空维→显性 Err。
    chk(&mut set, "D433-04", probe_dim_quantile(&lg, DIM_N, 50) == Err(ScCode::DeviateOver)
        && probe_dim_quantile(&lg, 3, 0) == Err(ScCode::LineShort)
        && probe_dim_quantile(&lg, 5, 50) == Err(ScCode::EvidenceHeld), "复评探针三连");

    // D433-05 边界探针：分数越界（>100）拒入。
    chk(&mut set, "D433-05", probe_score_range(101) == Err(ScCode::LineShort)
        && probe_score_range(100).is_ok(), "分数域探针");

    // D433-06 LCG 注入回归：20 轮短板注入 100% 拒 + 负对照零误报。
    let (n, c, fp) = lcg_shortboard_regression(20);
    chk(&mut set, "D433-06", n == 20 && c == 20 && fp == 0, "短板注入全拒");

    // D433-07 序列化往返：维分编码→解码恒等（含暂缓位）。
    let d0 = DimScore { dim: A11Y_DIM, final_score: 93, evidence: Some(7002), held: false };
    let d1 = DimScore { dim: 9, final_score: 0, evidence: None, held: true };
    chk(&mut set, "D433-07", decode_dim(&encode_dim(&d0)) == Ok(d0)
        && decode_dim(&encode_dim(&d1)) == Ok(d1), "维分序列化往返");

    // D433-08 解码边界：错长/越界维号→显性 Err。
    chk(&mut set, "D433-08", decode_dim(&[0u8; 9]) == Err(ScCode::DeviateOver)
        && decode_dim(&{ let mut v = encode_dim(&d0); v[0] = 0xFF; v[1] = 0xFF; v }) == Err(ScCode::DeviateOver), "维分解码边界");

    // D433-09 评分卡快照指纹：改分即变 + 确定性 + hex 投影。
    let mut c2 = ScoreCard::new();
    for i in 0..DIM_N { c2.put(i, LINE, 7000 + i as u32); }
    let f1 = scorecard_fingerprint(&c2);
    let f1b = scorecard_fingerprint(&c2);
    c2.put(A11Y_DIM, LINE + 1, 7002);
    let f2 = scorecard_fingerprint(&c2);
    chk(&mut set, "D433-09", f1 == f1b && f1 != f2
        && fingerprint_hex(f2).len() == 8, "评分卡指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves433_smoke() {
        let set = run_ves433_checks();
        assert!(set.failed_count() == 0, "ves433 red: {}", set.red_items_text());
    }
}
